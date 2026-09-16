use super::CellRef;

#[derive(Debug, Clone)]
pub(super) struct CellChange {
    pub(super) reference: CellRef,
    pub(super) before: String,
    pub(super) after: String,
}

#[derive(Debug, Clone)]
pub(super) struct EditCommand {
    pub(super) changes: Vec<CellChange>,
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
}

impl EditHistory {
    pub(super) fn is_dirty(&self) -> bool {
        self.current_state_id != self.saved_state_id
    }

    pub(super) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(super) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(super) fn record(&mut self, changes: Vec<CellChange>) {
        if changes.is_empty() {
            return;
        }

        self.next_state_id = self.next_state_id.saturating_add(1);
        let command = EditCommand {
            changes,
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
