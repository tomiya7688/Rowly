use super::CellRef;

#[derive(Debug, Clone)]
pub(super) struct CellChange {
    pub(super) reference: CellRef,
    pub(super) before: String,
    pub(super) after: String,
}

#[derive(Debug, Clone)]
pub(super) struct RowEdit {
    pub(super) index: usize,
    pub(super) removed: Vec<Vec<String>>,
    pub(super) inserted: Vec<Vec<String>>,
}

#[derive(Debug, Clone)]
pub(super) struct ColumnChange {
    pub(super) row: usize,
    pub(super) index: usize,
    pub(super) removed: Vec<String>,
    pub(super) inserted: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) enum EditOperation {
    Cells(Vec<CellChange>),
    Rows(RowEdit),
    Columns(Vec<ColumnChange>),
    Batch(Vec<EditOperation>),
}

impl EditOperation {
    fn is_empty(&self) -> bool {
        match self {
            Self::Cells(changes) => changes.is_empty(),
            Self::Rows(edit) => edit.removed.is_empty() && edit.inserted.is_empty(),
            Self::Columns(changes) => changes.is_empty(),
            Self::Batch(operations) => operations.iter().all(Self::is_empty),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct EditCommand {
    pub(super) operation: EditOperation,
    before_state_id: u64,
    after_state_id: u64,
}

#[derive(Debug, Default)]
pub(super) struct EditHistory {
    undo: Vec<EditCommand>,
    redo: Vec<EditCommand>,
    current_state_id: u64,
    saved_state_id: u64,
    next_state_id: u64,
    transaction: Option<Vec<EditOperation>>,
}

impl EditHistory {
    pub(super) fn is_dirty(&self) -> bool {
        self.current_state_id != self.saved_state_id
            || self
                .transaction
                .as_ref()
                .is_some_and(|operations| operations.iter().any(|operation| !operation.is_empty()))
    }

    pub(super) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(super) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(super) fn record(&mut self, operation: EditOperation) {
        if operation.is_empty() {
            return;
        }

        if let Some(transaction) = &mut self.transaction {
            transaction.push(operation);
            return;
        }

        self.record_committed(operation);
    }

    pub(super) fn begin_transaction(&mut self) -> bool {
        if self.transaction.is_some() {
            return false;
        }
        self.transaction = Some(Vec::new());
        true
    }

    pub(super) fn commit_transaction(&mut self) -> Option<Vec<EditOperation>> {
        let operations = self.transaction.take()?;
        let non_empty = operations
            .iter()
            .filter(|operation| !operation.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        if !non_empty.is_empty() {
            self.record_committed(EditOperation::Batch(non_empty));
        }
        Some(operations)
    }

    pub(super) fn take_transaction(&mut self) -> Option<Vec<EditOperation>> {
        self.transaction.take()
    }

    pub(super) fn restore_transaction(&mut self, operations: Vec<EditOperation>) {
        self.transaction = Some(operations);
    }

    pub(super) fn transaction_active(&self) -> bool {
        self.transaction.is_some()
    }

    fn record_committed(&mut self, operation: EditOperation) {
        self.next_state_id = self.next_state_id.saturating_add(1);
        let command = EditCommand {
            operation,
            before_state_id: self.current_state_id,
            after_state_id: self.next_state_id,
        };
        self.current_state_id = command.after_state_id;
        self.undo.push(command);
        self.redo.clear();
    }

    pub(super) fn take_undo(&mut self) -> Option<EditCommand> {
        self.undo.pop()
    }

    pub(super) fn commit_undo(&mut self, command: EditCommand) {
        self.current_state_id = command.before_state_id;
        self.redo.push(command);
    }

    pub(super) fn restore_undo(&mut self, command: EditCommand) {
        self.undo.push(command);
    }

    pub(super) fn take_redo(&mut self) -> Option<EditCommand> {
        self.redo.pop()
    }

    pub(super) fn commit_redo(&mut self, command: EditCommand) {
        self.current_state_id = command.after_state_id;
        self.undo.push(command);
    }

    pub(super) fn restore_redo(&mut self, command: EditCommand) {
        self.redo.push(command);
    }

    pub(super) fn mark_saved(&mut self) {
        self.saved_state_id = self.current_state_id;
    }
}
