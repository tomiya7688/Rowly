use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use csv::{ReaderBuilder, Terminator, WriterBuilder};
use thiserror::Error;

use crate::data::{
    ContentFingerprint, SourceEncoding, Table, fingerprint_file, read_csv, write_csv_utf8,
    write_csv_utf8_if_unchanged,
};

use super::{
    CellRange, CellRef, ColumnError, ColumnType, ReferenceError, ValidationReport, ValidationRule,
    ValidationTarget, ValidationViolation,
    history::{CellChange, ColumnChange, EditCommand, EditHistory, EditOperation, RowEdit},
    metadata::{ColumnMetadata, sidecar_path},
};

#[derive(Debug)]
pub struct CsvDocument {
    path: PathBuf,
    source_encoding: SourceEncoding,
    table: Table,
    history: EditHistory,
    validation_rules: BTreeMap<ValidationTarget, ValidationRule>,
    metadata: ColumnMetadata,
    metadata_error: Option<String>,
    disk_fingerprint: ContentFingerprint,
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
            table,
            history,
            validation_rules: BTreeMap::new(),
            metadata: ColumnMetadata::default(),
            metadata_error: None,
            disk_fingerprint,
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

        Ok(Self {
            path,
            source_encoding: loaded.encoding,
            table: loaded.table,
            history: EditHistory::default(),
            validation_rules: BTreeMap::new(),
            metadata,
            metadata_error,
            disk_fingerprint: loaded.fingerprint,
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

    /// Check for external edits. A clean document reloads changed content;
    /// dirty documents report a conflict and keep their in-memory edits.
    pub fn refresh_if_external_change(&mut self) -> Result<bool, DocumentError> {
        self.ensure_no_transaction("refresh external changes")?;
        let current =
            fingerprint_file(&self.path).map_err(|error| DocumentError::ExternalCheck {
                path: self.path.display().to_string(),
                message: error.to_string(),
            })?;
        if current == self.disk_fingerprint {
            return Ok(false);
        }
        if self.is_dirty() {
            return Err(DocumentError::ExternalModification {
                path: self.path.display().to_string(),
            });
        }
        self.reload_disk_content()?;
        Ok(true)
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

    /// Install a session-only validation rule for a column.
    ///
    /// A rule assignment is undoable but does not mark the CSV file dirty.
    pub fn set_validation_rule(
        &mut self,
        target: ValidationTarget,
        rule: ValidationRule,
    ) -> Result<(), DocumentError> {
        let column = self.resolve_validation_target(&target)?;
        let before = self.validation_rules.clone();
        let replaced = self
            .validation_rules
            .keys()
            .filter_map(|existing| {
                (self.resolve_validation_target(existing).ok() == Some(column))
                    .then_some(existing.clone())
            })
            .collect::<Vec<_>>();
        for existing in replaced {
            self.validation_rules.remove(&existing);
        }
        self.validation_rules.insert(target, rule);
        let after = self.validation_rules.clone();
        self.history
            .record(EditOperation::ValidationRules { before, after });
        Ok(())
    }

    pub fn remove_validation_rule(
        &mut self,
        target: &ValidationTarget,
    ) -> Result<bool, DocumentError> {
        let before = self.validation_rules.clone();
        if let Ok(column) = self.resolve_validation_target(target) {
            let matching = self
                .validation_rules
                .keys()
                .filter_map(|existing| {
                    (self.resolve_validation_target(existing).ok() == Some(column))
                        .then_some(existing.clone())
                })
                .collect::<Vec<_>>();
            for existing in matching {
                self.validation_rules.remove(&existing);
            }
        } else {
            self.validation_rules.remove(target);
        }
        let after = self.validation_rules.clone();
        if before == after {
            return Ok(false);
        }
        self.history
            .record(EditOperation::ValidationRules { before, after });
        Ok(true)
    }

    pub fn validation_rule(&self, target: &ValidationTarget) -> Option<&ValidationRule> {
        self.validation_rules.get(target)
    }

    pub fn validation_rules(&self) -> impl Iterator<Item = (&ValidationTarget, &ValidationRule)> {
        self.validation_rules.iter()
    }

    /// Return the ordered allowed values for the active rule on a column.
    pub fn allowed_values_for_column(&self, column: usize) -> Option<&[String]> {
        self.validation_rules.iter().find_map(|(target, rule)| {
            (self.resolve_validation_target(target).ok() == Some(column))
                .then(|| rule.allowed_values())
                .flatten()
        })
    }

    /// Inspect current data without rejecting it. This also reports legacy or
    /// externally supplied values that do not satisfy their session rules.
    pub fn validation_report(&self) -> ValidationReport {
        self.validation_report_for(&self.table)
    }

    fn validation_report_for(&self, table: &Table) -> ValidationReport {
        let mut report = ValidationReport::default();
        let mut rules_by_column =
            BTreeMap::<usize, Vec<(&ValidationTarget, &ValidationRule)>>::new();
        for (target, rule) in &self.validation_rules {
            if let Ok(column) = self.resolve_validation_target(target) {
                rules_by_column
                    .entry(column)
                    .or_default()
                    .push((target, rule));
            }
        }

        for row in 1..table.row_count() {
            for column in 0..table.column_count() {
                let Some(rules) = rules_by_column.get(&column) else {
                    continue;
                };
                let Some(value) = table.cell(row, column) else {
                    continue;
                };
                report.checked_cells += 1;
                for (target, rule) in rules {
                    if !rule.matches(value) {
                        report.violations.push(ValidationViolation {
                            cell: CellRef::new(row, column),
                            target: (*target).clone(),
                            value: value.to_owned(),
                            rule: (*rule).clone(),
                        });
                    }
                }
            }
        }
        report
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

    pub fn csv_text(&self) -> Result<String, DocumentError> {
        let mut writer = WriterBuilder::new()
            .terminator(Terminator::Any(b'\n'))
            .from_writer(Vec::new());
        for row in self.table.rows() {
            writer
                .write_record(row)
                .map_err(|error| DocumentError::Csv(error.to_string()))?;
        }
        let bytes = writer
            .into_inner()
            .map_err(|error| DocumentError::Csv(error.error().to_string()))?;
        String::from_utf8(bytes).map_err(|error| DocumentError::Csv(error.to_string()))
    }

    /// Parse an editable CSV buffer and atomically apply it after validating all
    /// data cells against the active session rules.
    pub fn apply_csv_text(&mut self, text: &str) -> Result<(), DocumentError> {
        let mut reader = ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(text.as_bytes());
        let mut rows = Vec::new();
        for record in reader.records() {
            let record = record.map_err(|error| DocumentError::Csv(error.to_string()))?;
            rows.push(record.iter().map(str::to_owned).collect());
        }
        self.replace_contents(rows)
    }

    pub fn replace_contents(&mut self, rows: Vec<Vec<String>>) -> Result<(), DocumentError> {
        let replacement = Table::new(rows);
        let before = self.table.rows().to_vec();
        let after = replacement.rows().to_vec();
        if before == after {
            return Ok(());
        }

        let report = self.validation_report_for(&replacement);
        if !report.is_valid() {
            return Err(DocumentError::ValidationRejected {
                violations: report.violations,
            });
        }

        self.table = replacement;
        self.history
            .record(EditOperation::Contents { before, after });
        Ok(())
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

        let before_validation = self.validation_rules.clone();
        let shifted_validation = self.shift_validation_rules_for_insert(index, count)?;
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

        self.validation_rules = shifted_validation;
        self.record_column_edit(changes, before_validation);
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
        if self.history.commit_transaction().is_none() {
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

        let before_validation = self.validation_rules.clone();
        let shifted_validation = self.shift_validation_rules_for_delete(index, end, count);
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

        self.validation_rules = shifted_validation;
        self.record_column_edit(changes, before_validation);
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
            if self.is_dirty() {
                return Err(DocumentError::ExternalModification {
                    path: self.path.display().to_string(),
                });
            }
            self.reload_disk_content()?;
            return Ok(());
        }
        self.disk_fingerprint =
            match write_csv_utf8_if_unchanged(&self.path, &self.table, Some(self.disk_fingerprint))
            {
                Ok(fingerprint) => fingerprint,
                Err(crate::data::CsvIoError::ExternalModification) => {
                    if self.is_dirty() {
                        return Err(DocumentError::ExternalModification {
                            path: self.path.display().to_string(),
                        });
                    }
                    self.reload_disk_content()?;
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
        self.history.mark_saved();
        Ok(())
    }

    fn reload_disk_content(&mut self) -> Result<(), DocumentError> {
        let loaded = read_csv(&self.path).map_err(|error| DocumentError::Open {
            path: self.path.display().to_string(),
            message: error.to_string(),
        })?;
        self.table = loaded.table;
        self.source_encoding = loaded.encoding;
        self.disk_fingerprint = loaded.fingerprint;
        self.history = EditHistory::default();
        Ok(())
    }

    fn resolve_validation_target(&self, target: &ValidationTarget) -> Result<usize, ColumnError> {
        match target {
            ValidationTarget::Index(column) if *column < self.column_count() => Ok(*column),
            ValidationTarget::Index(column) => Err(ColumnError::ColumnOutOfBounds {
                column: *column,
                column_count: self.column_count(),
            }),
            ValidationTarget::Header(header) => self.column_index_by_header(header),
        }
    }

    fn shift_validation_rules_for_insert(
        &self,
        index: usize,
        count: usize,
    ) -> Result<BTreeMap<ValidationTarget, ValidationRule>, DocumentError> {
        self.validation_rules
            .iter()
            .map(|(target, rule)| {
                let target = match target {
                    ValidationTarget::Index(column) if *column >= index => {
                        ValidationTarget::Index(column.checked_add(count).ok_or_else(|| {
                            DocumentError::Edit("validation column index overflowed".into())
                        })?)
                    }
                    _ => target.clone(),
                };
                Ok((target, rule.clone()))
            })
            .collect()
    }

    fn shift_validation_rules_for_delete(
        &self,
        index: usize,
        end: usize,
        count: usize,
    ) -> BTreeMap<ValidationTarget, ValidationRule> {
        self.validation_rules
            .iter()
            .filter_map(|(target, rule)| {
                let target = match target {
                    ValidationTarget::Index(column) if *column >= index && *column < end => {
                        return None;
                    }
                    ValidationTarget::Index(column) if *column >= end => {
                        ValidationTarget::Index(column - count)
                    }
                    _ => target.clone(),
                };
                Some((target, rule.clone()))
            })
            .collect()
    }

    fn record_column_edit(
        &mut self,
        columns: Vec<ColumnChange>,
        before_validation: BTreeMap<ValidationTarget, ValidationRule>,
    ) {
        let after_validation = self.validation_rules.clone();
        let operation = if before_validation == after_validation {
            EditOperation::Columns(columns)
        } else {
            EditOperation::Batch(vec![
                EditOperation::Columns(columns),
                EditOperation::ValidationRules {
                    before: before_validation,
                    after: after_validation,
                },
            ])
        };
        self.history.record(operation);
    }

    fn set_references_value(
        &mut self,
        references: impl IntoIterator<Item = CellRef>,
        value: String,
    ) -> Result<(), DocumentError> {
        let mut changes = Vec::new();
        let mut violations = Vec::new();

        for reference in references {
            let before = self.cell_ref(reference).ok_or_else(|| {
                DocumentError::Edit(format!(
                    "cell `{reference}` is outside the existing CSV table"
                ))
            })?;

            if before != value {
                if reference.row() > 0 {
                    for (target, rule) in &self.validation_rules {
                        if self.resolve_validation_target(target).ok() == Some(reference.column())
                            && !rule.matches(&value)
                        {
                            violations.push(ValidationViolation {
                                cell: reference,
                                target: target.clone(),
                                value: value.clone(),
                                rule: rule.clone(),
                            });
                        }
                    }
                }
                changes.push(CellChange {
                    reference,
                    before: before.to_owned(),
                    after: value.clone(),
                });
            }
        }

        if !violations.is_empty() {
            return Err(DocumentError::ValidationRejected { violations });
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
            EditOperation::Contents { before, after } => {
                self.apply_contents_change(before, after, direction)
            }
            EditOperation::ValidationRules { before, after } => {
                let (expected, replacement) = match direction {
                    CommandDirection::Undo => (after, before),
                    CommandDirection::Redo => (before, after),
                };
                if &self.validation_rules != expected {
                    return Err(DocumentError::Edit(
                        "edit history no longer matches the validation rules".into(),
                    ));
                }
                self.validation_rules = replacement.clone();
                Ok(())
            }
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

    fn apply_contents_change(
        &mut self,
        before: &[Vec<String>],
        after: &[Vec<String>],
        direction: CommandDirection,
    ) -> Result<(), DocumentError> {
        let (expected, replacement) = match direction {
            CommandDirection::Undo => (after, before),
            CommandDirection::Redo => (before, after),
        };
        if self.table.rows() != expected {
            return Err(DocumentError::Edit(
                "edit history no longer matches the table contents".into(),
            ));
        }
        self.table = Table::new(replacement.to_vec());
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

#[derive(Debug, Error)]
pub enum DocumentError {
    #[error("failed to open CSV `{path}`: {message}")]
    Open { path: String, message: String },

    #[error(transparent)]
    Reference(#[from] ReferenceError),

    #[error("failed to edit CSV: {0}")]
    Edit(String),

    #[error("invalid CSV text: {0}")]
    Csv(String),

    #[error("failed to save CSV `{path}`: {message}")]
    Save { path: String, message: String },

    #[error("failed to verify current CSV `{path}` before refresh/save: {message}")]
    ExternalCheck { path: String, message: String },

    #[error("CSV `{path}` changed outside Rowly after it was opened")]
    ExternalModification { path: String },

    #[error("invalid transaction operation: {0}")]
    Transaction(String),

    #[error("Rowly metadata operation failed: {0}")]
    Metadata(String),

    #[error("input validation rejected one or more cell values")]
    ValidationRejected {
        violations: Vec<ValidationViolation>,
    },

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
