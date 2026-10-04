use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::data::{
    ContentFingerprint, SourceEncoding, Table, fingerprint_file, read_csv, write_csv_utf8,
    write_csv_utf8_if_unchanged,
};

use super::{
    CellRange, CellRef, ColumnError, ColumnType, ReferenceError,
    history::{CellChange, ColumnChange, EditCommand, EditHistory, EditOperation, RowEdit},
    metadata::{ColumnMetadata, sidecar_path},
};

#[derive(Debug)]
pub struct CsvDocument {
    path: PathBuf,
    source_encoding: SourceEncoding,
    table: Table,
    baseline: Table,
    history: EditHistory,
    metadata: ColumnMetadata,
    metadata_error: Option<String>,
    disk_fingerprint: ContentFingerprint,
    external_conflicts: Vec<ExternalConflictDraft>,
}

/// A preserved three-way snapshot for local changes that could not be merged.
/// The current document remains the disk version plus any safe local cell edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalConflictDraft {
    pub baseline: Vec<Vec<String>>,
    pub local: Vec<Vec<String>>,
    pub disk: Vec<Vec<String>>,
    pub cell_conflicts: Vec<ExternalCellConflict>,
    pub structural_conflict: Option<ExternalStructureConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalCellConflict {
    pub row: usize,
    pub column: usize,
    pub baseline: String,
    pub local: String,
    pub disk: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalStructureConflict {
    RowCountChanged,
    RowShapeChanged,
    HeaderChanged,
    RowOrderChanged,
    DuplicateRowsAmbiguous,
}

impl CsvDocument {
    pub fn create(path: impl AsRef<Path>, rows: Vec<Vec<String>>) -> Result<Self, DocumentError> {
        let path = path.as_ref().to_path_buf();
        let table = Table::new(rows);
        let disk_fingerprint =
            write_csv_utf8(&path, &table).map_err(|error| DocumentError::Save {
                path: path.display().to_string(),
                message: error.to_string(),
            })?;

        let mut history = EditHistory::default();
        history.mark_saved();
        Ok(Self {
            path,
            source_encoding: SourceEncoding::Utf8,
            baseline: table.clone(),
            table,
            history,
            metadata: ColumnMetadata::default(),
            metadata_error: None,
            disk_fingerprint,
            external_conflicts: Vec::new(),
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, DocumentError> {
        let path = path.as_ref().to_path_buf();
        let loaded = read_csv(&path).map_err(|error| DocumentError::Open {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;

        let (metadata, metadata_error) = match ColumnMetadata::load(&path) {
            Ok(metadata) => (metadata, None),
            Err(error) => (ColumnMetadata::default(), Some(error.to_string())),
        };

        let baseline = loaded.table.clone();
        Ok(Self {
            path,
            source_encoding: loaded.encoding,
            table: loaded.table,
            baseline,
            history: EditHistory::default(),
            metadata,
            metadata_error,
            disk_fingerprint: loaded.fingerprint,
            external_conflicts: Vec::new(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source_encoding(&self) -> SourceEncoding {
        self.source_encoding
    }

    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }

    /// Synchronize external edits. Safe cell changes merge automatically;
    /// overlapping or structurally ambiguous local changes are preserved as
    /// three-way snapshots while the disk content becomes current.
    pub fn refresh_if_external_change(&mut self) -> Result<bool, DocumentError> {
        self.ensure_no_transaction("refresh external changes")?;
        self.merge_external_disk()
    }

    /// Preserved three-way snapshots for local changes that need later review.
    pub fn external_conflict_drafts(&self) -> &[ExternalConflictDraft] {
        &self.external_conflicts
    }

    /// Reapply the Local side of cell conflicts in a preserved draft.
    /// Structural conflicts require review and are not reapplied automatically.
    pub fn reapply_local_cell_conflicts(
        &mut self,
        draft_index: usize,
    ) -> Result<usize, DocumentError> {
        self.ensure_no_transaction("reapply conflict draft")?;
        let draft = self.external_conflicts.get(draft_index).ok_or_else(|| {
            DocumentError::ConflictDraft(format!("draft {draft_index} does not exist"))
        })?;
        if draft.structural_conflict.is_some() {
            return Err(DocumentError::ConflictDraft(
                "structural conflicts require manual review".into(),
            ));
        }

        let conflicts = draft.cell_conflicts.clone();
        let mut changes = Vec::new();
        for conflict in conflicts {
            let current = self
                .table
                .cell(conflict.row, conflict.column)
                .ok_or_else(|| {
                    DocumentError::ConflictDraft(format!(
                        "cell {},{} no longer exists",
                        conflict.row, conflict.column
                    ))
                })?;
            if current == conflict.local {
                continue;
            }
            if current != conflict.disk {
                return Err(DocumentError::ConflictDraft(format!(
                    "cell {},{} changed after the conflict was recorded",
                    conflict.row, conflict.column
                )));
            }
            changes.push((conflict.row, conflict.column, conflict.local));
        }

        if changes.is_empty() {
            return Ok(0);
        }
        self.begin_transaction()?;
        for (row, column, value) in &changes {
            if let Err(error) = self.set_cell(*row, *column, value.clone()) {
                let _ = self.rollback_transaction();
                return Err(error);
            }
        }
        self.commit_transaction()?;
        Ok(changes.len())
    }

    /// Replace the current table with a structural draft's Local snapshot.
    /// This is only allowed while the current table still matches that draft's Disk snapshot.
    pub fn reapply_local_structural_draft(
        &mut self,
        draft_index: usize,
    ) -> Result<usize, DocumentError> {
        self.ensure_no_transaction("reapply structural conflict draft")?;
        let draft = self.external_conflicts.get(draft_index).ok_or_else(|| {
            DocumentError::ConflictDraft(format!("draft {draft_index} does not exist"))
        })?;
        if draft.structural_conflict.is_none() {
            return Err(DocumentError::ConflictDraft(
                "draft does not contain a structural conflict".into(),
            ));
        }
        if self.table.rows() != draft.disk {
            return Err(DocumentError::ConflictDraft(
                "the current table changed after the structural conflict was recorded".into(),
            ));
        }

        let local = draft.local.clone();
        if local == self.table.rows() {
            return Ok(0);
        }
        self.begin_transaction()?;
        let removed = match self
            .table
            .replace_rows(0, self.table.row_count(), local.clone())
        {
            Ok(removed) => removed,
            Err(error) => {
                let _ = self.rollback_transaction();
                return Err(DocumentError::Edit(error.to_string()));
            }
        };
        self.history.record(EditOperation::Rows(RowEdit {
            index: 0,
            removed,
            inserted: local.clone(),
        }));
        self.commit_transaction()?;
        Ok(local.len())
    }

    pub fn metadata_path(&self) -> PathBuf {
        sidecar_path(&self.path)
    }

    pub fn metadata_error(&self) -> Option<&str> {
        self.metadata_error.as_deref()
    }

    pub fn set_column_type_declaration_by_header(
        &mut self,
        header: &str,
        column_type: ColumnType,
    ) -> Result<(), DocumentError> {
        self.column_index_by_header(header)?;
        self.metadata.set(header.to_owned(), column_type);
        self.metadata_error = None;
        Ok(())
    }

    pub fn remove_column_type_declaration_by_header(
        &mut self,
        header: &str,
    ) -> Result<bool, DocumentError> {
        self.column_index_by_header(header)?;
        Ok(self.metadata.remove(header))
    }

    pub fn column_type_declaration_by_header(
        &self,
        header: &str,
    ) -> Result<Option<ColumnType>, DocumentError> {
        self.column_index_by_header(header)?;
        Ok(self.metadata.get(header))
    }

    pub fn column_type_declarations(&self) -> impl Iterator<Item = (&str, ColumnType)> {
        self.metadata.declarations()
    }

    pub fn save_metadata(&mut self) -> Result<(), DocumentError> {
        self.metadata
            .save(&self.path)
            .map_err(|error| DocumentError::Metadata(error.to_string()))?;
        self.metadata_error = None;
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn row_count(&self) -> usize {
        self.table.row_count()
    }

    pub fn column_count(&self) -> usize {
        self.table.column_count()
    }

    pub fn rows(&self) -> impl Iterator<Item = &[String]> {
        self.table.rows().iter().map(Vec::as_slice)
    }

    pub fn cell(&self, row: usize, column: usize) -> Option<&str> {
        self.table.cell(row, column)
    }

    pub fn cell_ref(&self, reference: CellRef) -> Option<&str> {
        self.cell(reference.row(), reference.column())
    }

    pub fn cell_a1(&self, reference: &str) -> Result<Option<&str>, DocumentError> {
        Ok(self.cell_ref(reference.parse()?))
    }

    pub fn set_cell(
        &mut self,
        row: usize,
        column: usize,
        value: impl Into<String>,
    ) -> Result<(), DocumentError> {
        self.set_cell_ref(CellRef::new(row, column), value)
    }

    pub fn set_cell_ref(
        &mut self,
        reference: CellRef,
        value: impl Into<String>,
    ) -> Result<(), DocumentError> {
        self.set_references_value([reference], value.into())
    }

    pub fn set_cell_a1(
        &mut self,
        reference: &str,
        value: impl Into<String>,
    ) -> Result<(), DocumentError> {
        self.set_cell_ref(reference.parse()?, value)
    }

    pub fn set_range_value(
        &mut self,
        range: CellRange,
        value: impl Into<String>,
    ) -> Result<(), DocumentError> {
        self.set_references_value(range.iter(), value.into())
    }

    pub fn set_range_a1(
        &mut self,
        range: &str,
        value: impl Into<String>,
    ) -> Result<(), DocumentError> {
        self.set_range_value(range.parse()?, value)
    }

    pub fn insert_rows(&mut self, index: usize, count: usize) -> Result<(), DocumentError> {
        let width = self.column_count().max(1);
        let inserted = vec![vec![String::new(); width]; count];
        let removed = self
            .table
            .replace_rows(index, 0, inserted.clone())
            .map_err(|error| DocumentError::Edit(error.to_string()))?;

        self.history.record(EditOperation::Rows(RowEdit {
            index,
            removed,
            inserted,
        }));
        Ok(())
    }

    pub fn delete_rows(&mut self, index: usize, count: usize) -> Result<(), DocumentError> {
        let removed = self
            .table
            .replace_rows(index, count, Vec::new())
            .map_err(|error| DocumentError::Edit(error.to_string()))?;

        self.history.record(EditOperation::Rows(RowEdit {
            index,
            removed,
            inserted: Vec::new(),
        }));
        Ok(())
    }

    pub fn insert_columns(&mut self, index: usize, count: usize) -> Result<(), DocumentError> {
        let column_count = self.column_count();
        if index > column_count {
            return Err(DocumentError::Edit(format!(
                "column insertion index {index} is out of bounds for {column_count} columns"
            )));
        }

        let inserted = vec![String::new(); count];
        let changes = self
            .table
            .rows()
            .iter()
            .enumerate()
            .filter(|(_, row)| index <= row.len())
            .map(|(row, _)| ColumnChange {
                row,
                index,
                removed: Vec::new(),
                inserted: inserted.clone(),
            })
            .collect::<Vec<_>>();

        for change in &changes {
            self.table
                .replace_row_segment(change.row, change.index, 0, &change.inserted)
                .map_err(|error| DocumentError::Edit(error.to_string()))?;
        }

        self.history.record(EditOperation::Columns(changes));
        Ok(())
    }

    pub fn begin_transaction(&mut self) -> Result<(), DocumentError> {
        if !self.history.begin_transaction() {
            return Err(DocumentError::Transaction(
                "a transaction is already active".into(),
            ));
        }
        Ok(())
    }

    pub fn commit_transaction(&mut self) -> Result<(), DocumentError> {
        if !self.history.commit_transaction() {
            return Err(DocumentError::Transaction(
                "no transaction is active".into(),
            ));
        }
        Ok(())
    }

    pub fn rollback_transaction(&mut self) -> Result<(), DocumentError> {
        let Some(operations) = self.history.take_transaction() else {
            return Err(DocumentError::Transaction(
                "no transaction is active".into(),
            ));
        };

        for operation in operations.iter().rev() {
            if let Err(error) = self.apply_operation(operation, CommandDirection::Undo) {
                self.history.restore_transaction(operations);
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn transaction_active(&self) -> bool {
        self.history.transaction_active()
    }

    pub fn delete_columns(&mut self, index: usize, count: usize) -> Result<(), DocumentError> {
        let column_count = self.column_count();
        let end = index
            .checked_add(count)
            .ok_or_else(|| DocumentError::Edit("column range arithmetic overflowed".into()))?;
        if index > column_count || end > column_count {
            return Err(DocumentError::Edit(format!(
                "column range starting at {index} with length {count} exceeds {column_count} columns"
            )));
        }

        let changes = self
            .table
            .rows()
            .iter()
            .enumerate()
            .filter_map(|(row_index, row)| {
                if index >= row.len() {
                    return None;
                }

                let row_end = end.min(row.len());
                Some(ColumnChange {
                    row: row_index,
                    index,
                    removed: row[index..row_end].to_vec(),
                    inserted: Vec::new(),
                })
            })
            .collect::<Vec<_>>();

        for change in &changes {
            self.table
                .replace_row_segment(change.row, change.index, change.removed.len(), &[])
                .map_err(|error| DocumentError::Edit(error.to_string()))?;
        }

        self.history.record(EditOperation::Columns(changes));
        Ok(())
    }

    pub fn undo(&mut self) -> Result<bool, DocumentError> {
        self.ensure_no_transaction("undo")?;
        let Some(command) = self.history.take_undo() else {
            return Ok(false);
        };

        if let Err(error) = self.apply_command(&command, CommandDirection::Undo) {
            self.history.restore_undo(command);
            return Err(error);
        }

        self.history.commit_undo(command);
        Ok(true)
    }

    pub fn redo(&mut self) -> Result<bool, DocumentError> {
        self.ensure_no_transaction("redo")?;
        let Some(command) = self.history.take_redo() else {
            return Ok(false);
        };

        if let Err(error) = self.apply_command(&command, CommandDirection::Redo) {
            self.history.restore_redo(command);
            return Err(error);
        }

        self.history.commit_redo(command);
        Ok(true)
    }

    pub fn save(&mut self) -> Result<(), DocumentError> {
        self.ensure_no_transaction("save")?;
        let current =
            fingerprint_file(&self.path).map_err(|error| DocumentError::ExternalCheck {
                path: self.path.display().to_string(),
                message: error.to_string(),
            })?;
        if current != self.disk_fingerprint {
            self.merge_external_disk()?;
            if !self.is_dirty() {
                return Ok(());
            }
        }
        if !self.is_dirty() {
            return Ok(());
        }
        self.disk_fingerprint =
            match write_csv_utf8_if_unchanged(&self.path, &self.table, Some(self.disk_fingerprint))
            {
                Ok(fingerprint) => fingerprint,
                Err(crate::data::CsvIoError::ExternalModification) => {
                    self.merge_external_disk()?;
                    if self.is_dirty() {
                        return Err(DocumentError::ExternalModification {
                            path: self.path.display().to_string(),
                        });
                    }
                    return Ok(());
                }
                Err(error) => {
                    return Err(DocumentError::Save {
                        path: self.path.display().to_string(),
                        message: error.to_string(),
                    });
                }
            };

        self.source_encoding = SourceEncoding::Utf8;
        self.baseline = self.table.clone();
        self.history.mark_saved();
        Ok(())
    }

    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), DocumentError> {
        self.ensure_no_transaction("save_as")?;
        let path = path.as_ref().to_path_buf();
        self.disk_fingerprint =
            write_csv_utf8(&path, &self.table).map_err(|error| DocumentError::Save {
                path: path.display().to_string(),
                message: error.to_string(),
            })?;

        self.path = path;
        self.source_encoding = SourceEncoding::Utf8;
        self.baseline = self.table.clone();
        self.history.mark_saved();
        Ok(())
    }

    fn merge_external_disk(&mut self) -> Result<bool, DocumentError> {
        let loaded = read_csv(&self.path).map_err(|error| DocumentError::ExternalCheck {
            path: self.path.display().to_string(),
            message: error.to_string(),
        })?;
        if loaded.fingerprint == self.disk_fingerprint {
            return Ok(false);
        }

        let baseline = self.baseline.rows().to_vec();
        let local = self.table.rows().to_vec();
        let disk = loaded.table.rows().to_vec();

        if local == baseline {
            self.install_disk_snapshot(loaded);
            return Ok(true);
        }
        if local == disk {
            self.install_disk_snapshot(loaded);
            return Ok(true);
        }
        if disk == baseline {
            self.disk_fingerprint = loaded.fingerprint;
            self.source_encoding = loaded.encoding;
            self.baseline = loaded.table;
            return Ok(true);
        }

        match merge_row_only_changes(&baseline, &local, &disk) {
            Ok(merged) => {
                self.install_disk_snapshot(loaded);
                if merged != self.table.rows() {
                    self.begin_transaction()?;
                    let removed = self
                        .table
                        .replace_rows(
                            1,
                            self.table.row_count().saturating_sub(1),
                            merged[1..].to_vec(),
                        )
                        .map_err(|error| DocumentError::Edit(error.to_string()))?;
                    self.history.record(EditOperation::Rows(RowEdit {
                        index: 1,
                        removed,
                        inserted: merged[1..].to_vec(),
                    }));
                    self.commit_transaction()?;
                }
                return Ok(true);
            }
            Err(reason) if baseline.len() != local.len() || baseline.len() != disk.len() => {
                self.external_conflicts.push(ExternalConflictDraft {
                    baseline,
                    local,
                    disk,
                    cell_conflicts: Vec::new(),
                    structural_conflict: Some(reason),
                });
                self.install_disk_snapshot(loaded);
                return Ok(true);
            }
            Err(_) => {}
        }

        if let Some(reason) = structural_conflict(&baseline, &local, &disk) {
            self.external_conflicts.push(ExternalConflictDraft {
                baseline,
                local,
                disk,
                cell_conflicts: Vec::new(),
                structural_conflict: Some(reason),
            });
            self.install_disk_snapshot(loaded);
            return Ok(true);
        }

        let mut safe_local_changes = Vec::new();
        let mut conflicts = Vec::new();
        for row in 0..baseline.len() {
            for column in 0..baseline[row].len() {
                let before = &baseline[row][column];
                let local_value = &local[row][column];
                let disk_value = &disk[row][column];
                if local_value == before || local_value == disk_value {
                    continue;
                }
                if disk_value == before {
                    safe_local_changes.push((row, column, local_value.clone()));
                } else {
                    conflicts.push(ExternalCellConflict {
                        row,
                        column,
                        baseline: before.clone(),
                        local: local_value.clone(),
                        disk: disk_value.clone(),
                    });
                }
            }
        }

        if !conflicts.is_empty() {
            self.external_conflicts.push(ExternalConflictDraft {
                baseline,
                local,
                disk,
                cell_conflicts: conflicts,
                structural_conflict: None,
            });
        }

        self.install_disk_snapshot(loaded);
        if !safe_local_changes.is_empty() {
            self.begin_transaction()?;
            for (row, column, value) in safe_local_changes {
                if let Err(error) = self.set_cell(row, column, value) {
                    let _ = self.rollback_transaction();
                    return Err(error);
                }
            }
            self.commit_transaction()?;
        }
        Ok(true)
    }

    fn install_disk_snapshot(&mut self, loaded: crate::data::LoadedCsv) {
        self.table = loaded.table;
        self.baseline = self.table.clone();
        self.source_encoding = loaded.encoding;
        self.disk_fingerprint = loaded.fingerprint;
        self.history = EditHistory::default();
        self.history.mark_saved();
    }

    fn set_references_value(
        &mut self,
        references: impl IntoIterator<Item = CellRef>,
        value: String,
    ) -> Result<(), DocumentError> {
        let mut changes = Vec::new();

        for reference in references {
            let before = self.cell_ref(reference).ok_or_else(|| {
                DocumentError::Edit(format!(
                    "cell `{reference}` is outside the existing CSV table"
                ))
            })?;

            if before != value {
                changes.push(CellChange {
                    reference,
                    before: before.to_owned(),
                    after: value.clone(),
                });
            }
        }

        for change in &changes {
            self.table
                .set_cell(
                    change.reference.row(),
                    change.reference.column(),
                    change.after.clone(),
                )
                .map_err(|error| DocumentError::Edit(error.to_string()))?;
        }

        self.history.record(EditOperation::Cells(changes));
        Ok(())
    }

    fn apply_command(
        &mut self,
        command: &EditCommand,
        direction: CommandDirection,
    ) -> Result<(), DocumentError> {
        self.apply_operation(&command.operation, direction)
    }

    fn apply_operation(
        &mut self,
        operation: &EditOperation,
        direction: CommandDirection,
    ) -> Result<(), DocumentError> {
        match operation {
            EditOperation::Cells(changes) => self.apply_cell_changes(changes, direction),
            EditOperation::Rows(edit) => self.apply_row_edit(edit, direction),
            EditOperation::Columns(changes) => self.apply_column_changes(changes, direction),
            EditOperation::Batch(operations) => match direction {
                CommandDirection::Undo => {
                    for operation in operations.iter().rev() {
                        self.apply_operation(operation, direction)?;
                    }
                    Ok(())
                }
                CommandDirection::Redo => {
                    for operation in operations {
                        self.apply_operation(operation, direction)?;
                    }
                    Ok(())
                }
            },
        }
    }

    fn ensure_no_transaction(&self, operation: &str) -> Result<(), DocumentError> {
        if self.history.transaction_active() {
            return Err(DocumentError::Transaction(format!(
                "cannot {operation} while a transaction is active"
            )));
        }
        Ok(())
    }

    fn apply_cell_changes(
        &mut self,
        changes: &[CellChange],
        direction: CommandDirection,
    ) -> Result<(), DocumentError> {
        for change in changes {
            let value = match direction {
                CommandDirection::Undo => &change.before,
                CommandDirection::Redo => &change.after,
            };

            self.table
                .set_cell(
                    change.reference.row(),
                    change.reference.column(),
                    value.clone(),
                )
                .map_err(|error| {
                    DocumentError::Edit(format!(
                        "edit history no longer matches the table: {error}"
                    ))
                })?;
        }

        Ok(())
    }

    fn apply_row_edit(
        &mut self,
        edit: &RowEdit,
        direction: CommandDirection,
    ) -> Result<(), DocumentError> {
        let (expected, replacement) = match direction {
            CommandDirection::Undo => (&edit.inserted, &edit.removed),
            CommandDirection::Redo => (&edit.removed, &edit.inserted),
        };
        let end = edit
            .index
            .checked_add(expected.len())
            .ok_or_else(|| DocumentError::Edit("row history range overflowed".into()))?;
        let current = self.table.rows().get(edit.index..end).ok_or_else(|| {
            DocumentError::Edit("edit history no longer matches the table rows".into())
        })?;
        if current != expected.as_slice() {
            return Err(DocumentError::Edit(
                "edit history no longer matches the table rows".into(),
            ));
        }

        self.table
            .replace_rows(edit.index, expected.len(), replacement.to_vec())
            .map_err(|error| {
                DocumentError::Edit(format!("edit history no longer matches the table: {error}"))
            })?;
        Ok(())
    }

    fn apply_column_changes(
        &mut self,
        changes: &[ColumnChange],
        direction: CommandDirection,
    ) -> Result<(), DocumentError> {
        for change in changes {
            let expected = match direction {
                CommandDirection::Undo => &change.inserted,
                CommandDirection::Redo => &change.removed,
            };
            let row = self.table.rows().get(change.row).ok_or_else(|| {
                DocumentError::Edit("edit history no longer matches the table rows".into())
            })?;
            let end = change
                .index
                .checked_add(expected.len())
                .ok_or_else(|| DocumentError::Edit("column history range overflowed".into()))?;
            if row.get(change.index..end) != Some(expected.as_slice()) {
                return Err(DocumentError::Edit(
                    "edit history no longer matches the table columns".into(),
                ));
            }
        }

        for change in changes {
            let (expected, replacement) = match direction {
                CommandDirection::Undo => (&change.inserted, &change.removed),
                CommandDirection::Redo => (&change.removed, &change.inserted),
            };
            self.table
                .replace_row_segment(change.row, change.index, expected.len(), replacement)
                .map_err(|error| {
                    DocumentError::Edit(format!(
                        "edit history no longer matches the table: {error}"
                    ))
                })?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum CommandDirection {
    Undo,
    Redo,
}

fn structural_conflict(
    baseline: &[Vec<String>],
    local: &[Vec<String>],
    disk: &[Vec<String>],
) -> Option<ExternalStructureConflict> {
    if baseline.len() != local.len() || baseline.len() != disk.len() {
        return Some(ExternalStructureConflict::RowCountChanged);
    }
    if baseline.iter().enumerate().any(|(index, row)| {
        local.get(index).map(Vec::len) != Some(row.len())
            || disk.get(index).map(Vec::len) != Some(row.len())
    }) {
        return Some(ExternalStructureConflict::RowShapeChanged);
    }
    if baseline
        .first()
        .is_some_and(|header| local.first() != Some(header) || disk.first() != Some(header))
    {
        return Some(ExternalStructureConflict::HeaderChanged);
    }
    if row_order_changed(baseline, local) || row_order_changed(baseline, disk) {
        return Some(ExternalStructureConflict::RowOrderChanged);
    }
    if duplicate_rows_changed(baseline, local) || duplicate_rows_changed(baseline, disk) {
        return Some(ExternalStructureConflict::DuplicateRowsAmbiguous);
    }
    None
}

#[derive(Default)]
struct RowDelta {
    removed: HashSet<usize>,
    inserted: BTreeMap<usize, Vec<Vec<String>>>,
    cell_changes: BTreeMap<(usize, usize), String>,
}

fn merge_row_only_changes(
    baseline: &[Vec<String>],
    local: &[Vec<String>],
    disk: &[Vec<String>],
) -> Result<Vec<Vec<String>>, ExternalStructureConflict> {
    if baseline.is_empty() || local.first() != baseline.first() || disk.first() != baseline.first()
    {
        return Err(ExternalStructureConflict::HeaderChanged);
    }

    let local_delta = analyze_row_delta(baseline, local)?;
    let disk_delta = analyze_row_delta(baseline, disk)?;
    let mut removed = local_delta.removed.clone();
    removed.extend(disk_delta.removed.iter().copied());

    let mut insertions = local_delta.inserted.clone();
    for (slot, rows) in &disk_delta.inserted {
        match insertions.get(slot) {
            Some(local_rows) if local_rows != rows => {
                return Err(ExternalStructureConflict::DuplicateRowsAmbiguous);
            }
            Some(_) => {}
            None => {
                insertions.insert(*slot, rows.clone());
            }
        }
    }

    // An insertion next to a removed baseline row may be an edited replacement.
    if insertions.keys().any(|slot| {
        removed.contains(slot)
            || slot
                .checked_sub(1)
                .is_some_and(|index| removed.contains(&index))
    }) {
        return Err(ExternalStructureConflict::DuplicateRowsAmbiguous);
    }

    let mut merged = vec![baseline[0].clone()];
    let mut mapped_rows = HashMap::new();
    for (row_index, baseline_row) in baseline.iter().enumerate().skip(1) {
        if let Some(rows) = insertions.get(&row_index) {
            merged.extend(rows.iter().cloned());
        }
        if !removed.contains(&row_index) {
            mapped_rows.insert(row_index, merged.len());
            merged.push(baseline_row.clone());
        }
    }
    if let Some(rows) = insertions.get(&baseline.len()) {
        merged.extend(rows.iter().cloned());
    }
    for (&(row, column), value) in local_delta
        .cell_changes
        .iter()
        .chain(disk_delta.cell_changes.iter())
    {
        let Some(&merged_row) = mapped_rows.get(&row) else {
            return Err(ExternalStructureConflict::DuplicateRowsAmbiguous);
        };
        let cell = merged[merged_row]
            .get_mut(column)
            .ok_or(ExternalStructureConflict::RowShapeChanged)?;
        *cell = value.clone();
    }
    Ok(merged)
}

fn analyze_row_delta(
    baseline: &[Vec<String>],
    changed: &[Vec<String>],
) -> Result<RowDelta, ExternalStructureConflict> {
    if baseline.len() == changed.len()
        && baseline
            .iter()
            .zip(changed)
            .all(|(left, right)| left == right)
    {
        return Ok(RowDelta::default());
    }
    if baseline.len() == changed.len() {
        if row_order_changed(baseline, changed) || duplicate_rows_changed(baseline, changed) {
            return Err(ExternalStructureConflict::RowOrderChanged);
        }
        let mut delta = RowDelta::default();
        for (row_index, (before, after)) in baseline.iter().zip(changed).enumerate().skip(1) {
            if before.len() != after.len() {
                return Err(ExternalStructureConflict::RowShapeChanged);
            }
            for (column, (before, after)) in before.iter().zip(after).enumerate() {
                if before != after {
                    delta
                        .cell_changes
                        .insert((row_index, column), after.clone());
                }
            }
        }
        return Ok(delta);
    }
    let mut baseline_positions = HashMap::new();
    for (index, row) in baseline.iter().enumerate().skip(1) {
        if baseline_positions.insert(row.as_slice(), index).is_some() {
            return Err(ExternalStructureConflict::DuplicateRowsAmbiguous);
        }
    }
    let mut changed_counts = HashMap::<&[String], usize>::new();
    for row in changed.iter().skip(1) {
        *changed_counts.entry(row.as_slice()).or_default() += 1;
    }

    let mut delta = RowDelta::default();
    let mut next_baseline = 1;
    let mut pending_insertions = Vec::new();
    for row in changed.iter().skip(1) {
        match baseline_positions.get(row.as_slice()).copied() {
            Some(index) => {
                if changed_counts.get(row.as_slice()) != Some(&1) || index < next_baseline {
                    return Err(ExternalStructureConflict::RowOrderChanged);
                }
                if !pending_insertions.is_empty() {
                    delta
                        .inserted
                        .insert(index, std::mem::take(&mut pending_insertions));
                }
                delta.removed.extend(next_baseline..index);
                next_baseline = index + 1;
            }
            None => pending_insertions.push(row.clone()),
        }
    }
    if !pending_insertions.is_empty() {
        delta.inserted.insert(baseline.len(), pending_insertions);
    }
    delta.removed.extend(next_baseline..baseline.len());
    Ok(delta)
}

fn row_order_changed(baseline: &[Vec<String>], changed: &[Vec<String>]) -> bool {
    let baseline_counts = row_counts(baseline);
    let mut changed_positions = HashMap::<&[String], (usize, usize)>::new();
    for (index, row) in changed.iter().enumerate() {
        let entry = changed_positions.entry(row.as_slice()).or_default();
        entry.0 += 1;
        entry.1 = index;
    }
    baseline.iter().enumerate().any(|(index, row)| {
        baseline_counts.get(row.as_slice()) == Some(&1)
            && matches!(changed_positions.get(row.as_slice()), Some((1, position)) if *position != index)
    })
}

fn duplicate_rows_changed(baseline: &[Vec<String>], changed: &[Vec<String>]) -> bool {
    let baseline_counts = row_counts(baseline);
    baseline.iter().enumerate().any(|(index, row)| {
        baseline_counts
            .get(row.as_slice())
            .is_some_and(|count| *count > 1)
            && changed.get(index) != Some(row)
    })
}

fn row_counts(rows: &[Vec<String>]) -> HashMap<&[String], usize> {
    let mut counts = HashMap::new();
    for row in rows {
        *counts.entry(row.as_slice()).or_insert(0) += 1;
    }
    counts
}

#[derive(Debug, Error)]
pub enum DocumentError {
    #[error("failed to open CSV `{path}`: {message}")]
    Open { path: String, message: String },

    #[error(transparent)]
    Reference(#[from] ReferenceError),

    #[error("failed to edit CSV: {0}")]
    Edit(String),

    #[error("failed to save CSV `{path}`: {message}")]
    Save { path: String, message: String },

    #[error("failed to verify current CSV `{path}` before refresh/save: {message}")]
    ExternalCheck { path: String, message: String },

    #[error("CSV `{path}` changed outside Rowly after it was opened")]
    ExternalModification { path: String },

    #[error("cannot reapply external conflict draft: {0}")]
    ConflictDraft(String),

    #[error("invalid transaction operation: {0}")]
    Transaction(String),

    #[error("Rowly metadata operation failed: {0}")]
    Metadata(String),

    #[error(transparent)]
    Column(#[from] ColumnError),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use encoding_rs::SHIFT_JIS;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn create_writes_utf8_csv_and_starts_clean() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("created.csv");
        let rows = vec![
            vec!["名前".to_owned(), "値".to_owned()],
            vec!["田中".to_owned(), "1".to_owned()],
        ];

        let document = CsvDocument::create(&path, rows.clone()).unwrap();

        assert_eq!(document.source_encoding(), SourceEncoding::Utf8);
        assert!(!document.is_dirty());
        assert!(!document.can_undo());
        assert_eq!(
            document.rows().map(|row| row.to_vec()).collect::<Vec<_>>(),
            rows
        );

        let reopened = CsvDocument::open(&path).unwrap();
        assert_eq!(reopened.cell_a1("A2").unwrap(), Some("田中"));
        assert_eq!(reopened.cell_a1("B2").unwrap(), Some("1"));
    }

    #[test]
    fn column_type_metadata_round_trips_without_changing_csv() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        let source = "名前,年齢\n田中,20\n山田,21\n";
        fs::write(&path, source).unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document
            .set_column_type_declaration_by_header("年齢", ColumnType::Integer)
            .unwrap();
        document.save_metadata().unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), source);
        assert!(document.metadata_path().exists());

        let reopened = CsvDocument::open(&path).unwrap();
        assert_eq!(
            reopened.column_type_declaration_by_header("年齢").unwrap(),
            Some(ColumnType::Integer)
        );
        assert_eq!(reopened.metadata_error(), None);
        assert!(!reopened.is_dirty());
    }

    #[test]
    fn column_type_metadata_follows_header_after_column_reorder() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "名前,年齢\n田中,20\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document
            .set_column_type_declaration_by_header("年齢", ColumnType::Integer)
            .unwrap();
        document.insert_columns(0, 1).unwrap();
        document.set_cell(0, 0, "ID").unwrap();

        assert_eq!(document.column_index_by_header("年齢").unwrap(), 2);
        assert_eq!(
            document.column_type_declaration_by_header("年齢").unwrap(),
            Some(ColumnType::Integer)
        );
    }

    #[test]
    fn duplicate_header_cannot_receive_type_declaration() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "名前,名前\n田中,山田\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        assert!(matches!(
            document.set_column_type_declaration_by_header("名前", ColumnType::String),
            Err(DocumentError::Column(ColumnError::AmbiguousHeader { .. }))
        ));
    }

    #[test]
    fn invalid_sidecar_does_not_prevent_csv_open() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "名前,年齢\n田中,20\n").unwrap();
        fs::write(
            sidecar_path(&path),
            r#"{"version":1,"columns":{"年齢":{"type":"Unknown"}}}"#,
        )
        .unwrap();

        let document = CsvDocument::open(&path).unwrap();

        assert_eq!(document.cell_a1("B2").unwrap(), Some("20"));
        assert!(document.metadata_error().is_some());
        assert_eq!(
            document.column_type_declaration_by_header("年齢").unwrap(),
            None
        );
    }

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
    fn a1_range_edit_is_one_undoable_command() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "value\n1\n2\n3\n4\n5\n6\n7\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.set_range_a1("A1:A8", "8").unwrap();

        for row in 0..8 {
            assert_eq!(document.cell(row, 0), Some("8"));
        }
        assert!(document.can_undo());
        assert!(!document.can_redo());

        assert!(document.undo().unwrap());
        assert_eq!(document.cell_a1("A1").unwrap(), Some("value"));
        assert_eq!(document.cell_a1("A2").unwrap(), Some("1"));
        assert_eq!(document.cell_a1("A8").unwrap(), Some("7"));
        assert!(document.can_redo());

        assert!(document.redo().unwrap());
        for row in 0..8 {
            assert_eq!(document.cell(row, 0), Some("8"));
        }
    }

    #[test]
    fn range_edit_validates_every_cell_before_writing() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("ragged.csv");
        fs::write(&path, "a,b\nc\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        let error = document.set_range_a1("A1:B2", "x").unwrap_err();

        assert!(error.to_string().contains("B2"));
        assert_eq!(document.cell_a1("A1").unwrap(), Some("a"));
        assert_eq!(document.cell_a1("B1").unwrap(), Some("b"));
        assert_eq!(document.cell_a1("A2").unwrap(), Some("c"));
        assert!(!document.can_undo());
        assert!(!document.is_dirty());
    }

    #[test]
    fn row_insert_delete_and_history_restore_exact_rows() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("rows.csv");
        fs::write(&path, "a,b\n1,2\n3,4\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.insert_rows(1, 2).unwrap();
        assert_eq!(document.row_count(), 5);
        assert_eq!(document.cell(1, 0), Some(""));
        assert_eq!(document.cell(1, 1), Some(""));
        assert_eq!(document.cell(3, 0), Some("1"));

        assert!(document.undo().unwrap());
        assert_eq!(document.row_count(), 3);
        assert_eq!(document.cell(1, 0), Some("1"));
        assert!(document.redo().unwrap());
        assert_eq!(document.row_count(), 5);

        document.delete_rows(1, 3).unwrap();
        assert_eq!(document.row_count(), 2);
        assert_eq!(document.cell(1, 0), Some("3"));
        assert!(document.undo().unwrap());
        assert_eq!(document.row_count(), 5);
        assert_eq!(document.cell(3, 0), Some("1"));
        assert_eq!(document.cell(4, 0), Some("3"));
    }

    #[test]
    fn column_insert_preserves_ragged_missing_cells_and_undo_restores_shape() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("ragged.csv");
        fs::write(&path, "a,b,c\n1,2\nx\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.insert_columns(2, 1).unwrap();

        assert_eq!(document.column_count(), 4);
        assert_eq!(document.cell(0, 2), Some(""));
        assert_eq!(document.cell(0, 3), Some("c"));
        assert_eq!(document.cell(1, 2), Some(""));
        assert_eq!(document.cell(2, 1), None);

        assert!(document.undo().unwrap());
        assert_eq!(document.column_count(), 3);
        assert_eq!(document.cell(0, 2), Some("c"));
        assert_eq!(document.cell(1, 1), Some("2"));
        assert_eq!(document.cell(1, 2), None);
        assert_eq!(document.cell(2, 1), None);

        assert!(document.redo().unwrap());
        assert_eq!(document.cell(0, 3), Some("c"));
    }

    #[test]
    fn column_delete_and_undo_restore_removed_values_per_row() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("ragged.csv");
        fs::write(&path, "a,b,c,d\n1,2,3\nx,y\nz\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.delete_columns(1, 2).unwrap();

        assert_eq!(document.cell(0, 0), Some("a"));
        assert_eq!(document.cell(0, 1), Some("d"));
        assert_eq!(document.cell(1, 0), Some("1"));
        assert_eq!(document.cell(1, 1), None);
        assert_eq!(document.cell(2, 0), Some("x"));
        assert_eq!(document.cell(2, 1), None);
        assert_eq!(document.cell(3, 0), Some("z"));

        assert!(document.undo().unwrap());
        assert_eq!(document.cell(0, 1), Some("b"));
        assert_eq!(document.cell(0, 2), Some("c"));
        assert_eq!(document.cell(0, 3), Some("d"));
        assert_eq!(document.cell(1, 1), Some("2"));
        assert_eq!(document.cell(1, 2), Some("3"));
        assert_eq!(document.cell(2, 1), Some("y"));
        assert_eq!(document.cell(3, 1), None);
    }

    #[test]
    fn invalid_structural_edit_is_atomic_and_not_recorded() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        assert!(document.delete_rows(1, 2).is_err());
        assert!(document.delete_columns(1, 2).is_err());

        assert_eq!(document.row_count(), 2);
        assert_eq!(document.column_count(), 2);
        assert_eq!(document.cell(1, 1), Some("2"));
        assert!(!document.can_undo());
        assert!(!document.is_dirty());
    }

    #[test]
    fn dirty_state_tracks_saved_state_through_undo_and_redo() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "name,value\nAlice,1\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.set_cell_a1("B2", "2").unwrap();
        assert!(document.is_dirty());

        assert!(document.undo().unwrap());
        assert!(!document.is_dirty());

        assert!(document.redo().unwrap());
        assert!(document.is_dirty());
        document.save().unwrap();
        assert!(!document.is_dirty());

        assert!(document.undo().unwrap());
        assert!(document.is_dirty());
        assert!(document.redo().unwrap());
        assert!(!document.is_dirty());
    }

    #[test]
    fn structural_edit_uses_same_dirty_history_state() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.insert_columns(1, 1).unwrap();
        assert!(document.is_dirty());

        assert!(document.undo().unwrap());
        assert!(!document.is_dirty());
        assert!(document.redo().unwrap());
        assert!(document.is_dirty());
    }

    #[test]
    fn new_edit_after_undo_discards_redo_branch() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.set_cell_a1("A2", "x").unwrap();
        document.set_cell_a1("B2", "y").unwrap();
        assert!(document.undo().unwrap());
        assert!(document.can_redo());

        document.set_cell_a1("A1", "header").unwrap();
        assert!(!document.can_redo());
    }

    #[test]
    fn committed_transaction_is_one_undoable_command() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n3,4\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.begin_transaction().unwrap();
        document.set_cell_a1("A2", "x").unwrap();
        document.set_cell_a1("B3", "y").unwrap();
        document.insert_rows(2, 1).unwrap();
        assert!(document.is_dirty());
        assert!(document.transaction_active());
        document.commit_transaction().unwrap();

        assert!(!document.transaction_active());
        assert_eq!(document.cell_a1("A2").unwrap(), Some("x"));
        assert_eq!(document.row_count(), 4);

        assert!(document.undo().unwrap());
        assert_eq!(document.cell_a1("A2").unwrap(), Some("1"));
        assert_eq!(document.cell_a1("B3").unwrap(), Some("4"));
        assert_eq!(document.row_count(), 3);
        assert!(!document.can_undo());

        assert!(document.redo().unwrap());
        assert_eq!(document.cell_a1("A2").unwrap(), Some("x"));
        assert_eq!(document.row_count(), 4);
    }

    #[test]
    fn rollback_transaction_restores_all_changes_without_history() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.begin_transaction().unwrap();
        document.set_cell_a1("A2", "x").unwrap();
        document.insert_columns(1, 1).unwrap();
        assert!(document.is_dirty());

        document.rollback_transaction().unwrap();

        assert_eq!(document.cell_a1("A2").unwrap(), Some("1"));
        assert_eq!(document.cell_a1("B2").unwrap(), Some("2"));
        assert_eq!(document.column_count(), 2);
        assert!(!document.is_dirty());
        assert!(!document.can_undo());
        assert!(!document.transaction_active());
    }

    #[test]
    fn transaction_rejects_nested_begin_and_history_or_save_operations() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.begin_transaction().unwrap();
        assert!(matches!(
            document.begin_transaction(),
            Err(DocumentError::Transaction(_))
        ));
        assert!(matches!(
            document.undo(),
            Err(DocumentError::Transaction(_))
        ));
        assert!(matches!(
            document.redo(),
            Err(DocumentError::Transaction(_))
        ));
        assert!(matches!(
            document.save(),
            Err(DocumentError::Transaction(_))
        ));
        assert!(matches!(
            document.save_as(directory.path().join("other.csv")),
            Err(DocumentError::Transaction(_))
        ));

        document.rollback_transaction().unwrap();
    }

    #[test]
    fn transaction_commit_and_rollback_require_active_transaction() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        assert!(matches!(
            document.commit_transaction(),
            Err(DocumentError::Transaction(_))
        ));
        assert!(matches!(
            document.rollback_transaction(),
            Err(DocumentError::Transaction(_))
        ));
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
