use eframe::egui;
use std::path::PathBuf;

use crate::process::{CellRef, CsvDocument};

const CELL_WIDTH: f32 = 120.0;
const ROW_HEIGHT: f32 = 26.0;
const GUTTER_WIDTH: f32 = 56.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum WorkspaceMode {
    #[default]
    TableEditor,
    TextEditor,
    Viewer,
}

pub struct RowlyApp {
    mode: WorkspaceMode,
    path_input: String,
    reference_input: String,
    document: Option<CsvDocument>,
    status: String,
    zoom: f32,
    selection: CellRef,
    selection_anchor: CellRef,
    editing: bool,
    edit_value: String,
}

impl Default for RowlyApp {
    fn default() -> Self {
        Self {
            mode: WorkspaceMode::TableEditor,
            path_input: String::new(),
            reference_input: "A1".to_owned(),
            document: None,
            status: "CSV ファイルを開いてください".to_owned(),
            zoom: 100.0,
            selection: CellRef::new(0, 0),
            selection_anchor: CellRef::new(0, 0),
            editing: false,
            edit_value: String::new(),
        }
    }
}

impl RowlyApp {
    fn open_csv(&mut self) {
        let path = PathBuf::from(self.path_input.trim());
        match CsvDocument::open(&path) {
            Ok(document) => {
                self.status = format!("{} を開きました", path.display());
                self.path_input = path.display().to_string();
                self.document = Some(document);
                self.selection = CellRef::new(0, 0);
                self.selection_anchor = self.selection;
                self.reference_input = self.selection.to_string();
                self.editing = false;
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn save_csv(&mut self) {
        let Some(document) = self.document.as_mut() else {
            self.status = "保存する CSV がありません".to_owned();
            return;
        };
        match document.save() {
            Ok(()) => self.status = format!("{} を保存しました", document.path().display()),
            Err(error) => self.status = error.to_string(),
        }
    }

    fn show_menu(&mut self, ctx: &egui::Context) {
        let mut open_requested = false;
        let mut save_requested = false;
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open CSV…").clicked() {
                        open_requested = true;
                        ui.close_menu();
                    }
                    if ui.button("Save").clicked() {
                        save_requested = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    let can_undo = self.document.as_ref().is_some_and(CsvDocument::can_undo);
                    let can_redo = self.document.as_ref().is_some_and(CsvDocument::can_redo);
                    if ui
                        .add_enabled(can_undo, egui::Button::new("Undo"))
                        .clicked()
                    {
                        self.undo();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(can_redo, egui::Button::new("Redo"))
                        .clicked()
                    {
                        self.redo();
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.label("Choose a workspace mode from the toolbar.");
                });
                ui.separator();
                ui.label("Rowly");
            });
        });
        if open_requested {
            self.open_csv();
        }
        if save_requested {
            self.save_csv();
        }
    }

    fn show_toolbar(&mut self, ctx: &egui::Context) {
        let mut open_requested = false;
        let mut save_requested = false;
        let mut undo_requested = false;
        let mut redo_requested = false;
        let mut insert_row_requested = false;
        let mut delete_row_requested = false;
        let mut insert_column_requested = false;
        let mut delete_column_requested = false;
        let mut navigate_requested = false;
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("CSV");
                ui.add(
                    egui::TextEdit::singleline(&mut self.path_input)
                        .hint_text("CSV file path")
                        .desired_width(360.0),
                );
                if ui.button("Open").clicked() {
                    open_requested = true;
                }
                if ui
                    .add_enabled(self.document.is_some(), egui::Button::new("Save"))
                    .clicked()
                {
                    save_requested = true;
                }
                let can_undo = self.document.as_ref().is_some_and(CsvDocument::can_undo);
                let can_redo = self.document.as_ref().is_some_and(CsvDocument::can_redo);
                if ui
                    .add_enabled(can_undo, egui::Button::new("Undo"))
                    .clicked()
                {
                    undo_requested = true;
                }
                if ui
                    .add_enabled(can_redo, egui::Button::new("Redo"))
                    .clicked()
                {
                    redo_requested = true;
                }
                ui.separator();
                ui.label("Cell");
                let address = ui
                    .add(egui::TextEdit::singleline(&mut self.reference_input).desired_width(58.0));
                if address.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    navigate_requested = true;
                }
                if ui.button("Go").clicked() {
                    navigate_requested = true;
                }
                if self.mode == WorkspaceMode::TableEditor {
                    ui.separator();
                    if ui.button("Insert row").clicked() {
                        insert_row_requested = true;
                    }
                    if ui.button("Delete row").clicked() {
                        delete_row_requested = true;
                    }
                    if ui.button("Insert column").clicked() {
                        insert_column_requested = true;
                    }
                    if ui.button("Delete column").clicked() {
                        delete_column_requested = true;
                    }
                }
                ui.separator();
                ui.selectable_value(&mut self.mode, WorkspaceMode::TableEditor, "Table Editor");
                ui.selectable_value(&mut self.mode, WorkspaceMode::TextEditor, "Text Editor");
                ui.selectable_value(&mut self.mode, WorkspaceMode::Viewer, "Viewer");
            });
        });
        if open_requested {
            self.open_csv();
        }
        if save_requested {
            self.save_csv();
        }
        if undo_requested {
            self.undo();
        }
        if redo_requested {
            self.redo();
        }
        if insert_row_requested {
            self.insert_row();
        }
        if delete_row_requested {
            self.delete_row();
        }
        if insert_column_requested {
            self.insert_column();
        }
        if delete_column_requested {
            self.delete_column();
        }
        if navigate_requested {
            self.navigate_to_cell();
        }
    }

    fn show_workspace(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.document.is_none() {
                ui.vertical_centered(|ui| {
                    ui.add_space(120.0);
                    ui.heading("Rowly");
                    ui.label("CSV を開くと、ここにワークスペースが表示されます。");
                });
                return;
            }

            match self.mode {
                WorkspaceMode::TableEditor => {
                    ui.heading("Table Editor");
                    self.show_editor_grid(ui);
                }
                WorkspaceMode::TextEditor => {
                    let document = self.document.as_ref().expect("document checked above");
                    ui.heading("Text Editor");
                    ui.label("テキスト編集領域はこのシェルでは表示用です。");
                    let mut text = document
                        .rows()
                        .map(|row| row.join(","))
                        .collect::<Vec<_>>()
                        .join("\n");
                    egui::ScrollArea::both().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut text)
                                .font(egui::TextStyle::Monospace)
                                .interactive(false)
                                .desired_width(f32::INFINITY)
                                .desired_rows(20),
                        );
                    });
                }
                WorkspaceMode::Viewer => {
                    let document = self.document.as_ref().expect("document checked above");
                    ui.heading("Viewer");
                    self.show_table_preview(ui, document, true);
                }
            }
        });
    }

    fn show_editor_grid(&mut self, ui: &mut egui::Ui) {
        let document = self.document.as_ref().expect("document checked above");
        let row_count = document.row_count();
        let column_count = document.column_count();
        let total_width = GUTTER_WIDTH + column_count as f32 * CELL_WIDTH;
        let mut commit_value = None;

        if row_count == 0 || column_count == 0 {
            ui.label("CSVにセルがありません。行や列を挿入して編集を開始できます。");
        }

        egui::ScrollArea::horizontal()
            .id_salt("table_grid_horizontal")
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                ui.set_min_width(total_width.max(viewport.width()));
                let first_column = (((viewport.min.x - GUTTER_WIDTH).max(0.0) / CELL_WIDTH).floor()
                    as usize)
                    .min(column_count);
                let last_column = (((viewport.max.x - GUTTER_WIDTH).max(0.0) / CELL_WIDTH).ceil()
                    as usize)
                    .saturating_add(1)
                    .min(column_count);

                ui.horizontal(|ui| {
                    ui.add_sized([GUTTER_WIDTH, ROW_HEIGHT], egui::Label::new("#"));
                    ui.add_space(first_column as f32 * CELL_WIDTH);
                    for column in first_column..last_column {
                        let label = column_label(column);
                        let selected = self.selection.column() == column;
                        if ui
                            .add_sized(
                                [CELL_WIDTH, ROW_HEIGHT],
                                egui::Button::new(label).selected(selected),
                            )
                            .clicked()
                        {
                            let row = self.selection.row();
                            update_selection(
                                &mut self.selection,
                                &mut self.selection_anchor,
                                &mut self.reference_input,
                                &mut self.editing,
                                CellRef::new(row, column),
                                false,
                                (row_count, column_count),
                            );
                        }
                    }
                });

                egui::ScrollArea::vertical()
                    .id_salt("table_grid_vertical")
                    .auto_shrink([false, false])
                    .show_rows(ui, ROW_HEIGHT, row_count, |ui, visible_rows| {
                        for row in visible_rows {
                            ui.horizontal(|ui| {
                                let row_header = ui.add_sized(
                                    [GUTTER_WIDTH, ROW_HEIGHT],
                                    egui::Button::new((row + 1).to_string())
                                        .selected(self.selection.row() == row),
                                );
                                if row_header.clicked() {
                                    let column = self.selection.column();
                                    update_selection(
                                        &mut self.selection,
                                        &mut self.selection_anchor,
                                        &mut self.reference_input,
                                        &mut self.editing,
                                        CellRef::new(row, column),
                                        false,
                                        (row_count, column_count),
                                    );
                                }
                                ui.add_space(first_column as f32 * CELL_WIDTH);

                                for column in first_column..last_column {
                                    let reference = CellRef::new(row, column);
                                    let value = document.cell(row, column);
                                    let range = cell_range(self.selection_anchor, self.selection);
                                    let selected = row >= range.0
                                        && row <= range.1
                                        && column >= range.2
                                        && column <= range.3;

                                    if self.editing
                                        && self.selection == reference
                                        && value.is_some()
                                    {
                                        let response = ui.add_sized(
                                            [CELL_WIDTH, ROW_HEIGHT],
                                            egui::TextEdit::singleline(&mut self.edit_value),
                                        );
                                        response.request_focus();
                                        if response.lost_focus()
                                            && ui.input(|input| input.key_pressed(egui::Key::Enter))
                                        {
                                            commit_value =
                                                Some((row, column, self.edit_value.clone()));
                                            self.editing = false;
                                        }
                                        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                                            self.editing = false;
                                        }
                                    } else {
                                        if let Some(value) = value {
                                            let response = ui.add_sized(
                                                [CELL_WIDTH, ROW_HEIGHT],
                                                egui::Button::new(value).selected(selected),
                                            );
                                            if response.clicked() {
                                                let extend =
                                                    ui.input(|input| input.modifiers.shift);
                                                update_selection(
                                                    &mut self.selection,
                                                    &mut self.selection_anchor,
                                                    &mut self.reference_input,
                                                    &mut self.editing,
                                                    reference,
                                                    extend,
                                                    (row_count, column_count),
                                                );
                                            }
                                            if response.double_clicked() {
                                                update_selection(
                                                    &mut self.selection,
                                                    &mut self.selection_anchor,
                                                    &mut self.reference_input,
                                                    &mut self.editing,
                                                    reference,
                                                    false,
                                                    (row_count, column_count),
                                                );
                                                self.edit_value = value.to_owned();
                                                self.editing = true;
                                            }
                                        } else {
                                            ui.add_enabled_ui(false, |ui| {
                                                ui.add_sized(
                                                    [CELL_WIDTH, ROW_HEIGHT],
                                                    egui::Button::new(""),
                                                );
                                            });
                                        }
                                    }
                                }
                            });
                        }
                    });
            });

        if let Some((row, column, value)) = commit_value {
            self.set_cell(row, column, value);
        }
    }

    fn set_selection(&mut self, reference: CellRef, extend: bool) {
        let (row_count, column_count) = self.document.as_ref().map_or((0, 0), |document| {
            (document.row_count(), document.column_count())
        });
        update_selection(
            &mut self.selection,
            &mut self.selection_anchor,
            &mut self.reference_input,
            &mut self.editing,
            reference,
            extend,
            (row_count, column_count),
        );
    }

    fn begin_edit(&mut self, value: &str) {
        self.edit_value = value.to_owned();
        self.editing = true;
    }

    fn set_cell(&mut self, row: usize, column: usize, value: String) {
        let Some(document) = self.document.as_mut() else {
            return;
        };
        match document.set_cell(row, column, value) {
            Ok(()) => self.status = format!("Cell updated ({})", self.selection),
            Err(error) => self.status = error.to_string(),
        }
    }

    fn navigate_to_cell(&mut self) {
        match self.reference_input.parse::<CellRef>() {
            Ok(reference) => {
                self.set_selection(reference, false);
                self.status = format!("Selected {}", self.selection);
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn insert_row(&mut self) {
        let result = self
            .document
            .as_mut()
            .map(|document| document.insert_rows(self.selection.row(), 1));
        self.apply_structure_result(result, "Inserted row");
    }

    fn delete_row(&mut self) {
        let result = self
            .document
            .as_mut()
            .map(|document| document.delete_rows(self.selection.row(), 1));
        self.apply_structure_result(result, "Deleted row");
    }

    fn insert_column(&mut self) {
        let result = self
            .document
            .as_mut()
            .map(|document| document.insert_columns(self.selection.column(), 1));
        self.apply_structure_result(result, "Inserted column");
    }

    fn delete_column(&mut self) {
        let result = self
            .document
            .as_mut()
            .map(|document| document.delete_columns(self.selection.column(), 1));
        self.apply_structure_result(result, "Deleted column");
    }

    fn apply_structure_result(
        &mut self,
        result: Option<Result<(), crate::process::DocumentError>>,
        success: &str,
    ) {
        match result {
            Some(Ok(())) => {
                self.status = success.to_owned();
                self.clamp_selection();
            }
            Some(Err(error)) => self.status = error.to_string(),
            None => self.status = "CSV ファイルを開いてください".to_owned(),
        }
        self.editing = false;
    }

    fn clamp_selection(&mut self) {
        let Some(document) = self.document.as_ref() else {
            return;
        };
        let row = self
            .selection
            .row()
            .min(document.row_count().saturating_sub(1));
        let column = self
            .selection
            .column()
            .min(document.column_count().saturating_sub(1));
        self.set_selection(CellRef::new(row, column), false);
    }

    fn undo(&mut self) {
        match self.document.as_mut().map(CsvDocument::undo) {
            Some(Ok(true)) => {
                self.status = "Undone".to_owned();
                self.clamp_selection();
            }
            Some(Ok(false)) => self.status = "Nothing to undo".to_owned(),
            Some(Err(error)) => self.status = error.to_string(),
            None => {}
        }
        self.editing = false;
    }

    fn redo(&mut self) {
        match self.document.as_mut().map(CsvDocument::redo) {
            Some(Ok(true)) => {
                self.status = "Redone".to_owned();
                self.clamp_selection();
            }
            Some(Ok(false)) => self.status = "Nothing to redo".to_owned(),
            Some(Err(error)) => self.status = error.to_string(),
            None => {}
        }
        self.editing = false;
    }

    fn show_table_preview(&self, ui: &mut egui::Ui, document: &CsvDocument, header: bool) {
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("csv_preview")
                .striped(true)
                .min_col_width(80.0)
                .show(ui, |ui| {
                    for (row_index, row) in document.rows().enumerate() {
                        ui.weak((row_index + 1).to_string());
                        for value in row {
                            if header && row_index == 0 {
                                ui.strong(value);
                            } else {
                                ui.label(value);
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    }

    fn show_status(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                if let Some(document) = self.document.as_ref() {
                    ui.separator();
                    ui.label(format!(
                        "{} rows × {} columns",
                        document.row_count(),
                        document.column_count()
                    ));
                    ui.label(if document.is_dirty() {
                        "Modified"
                    } else {
                        "Saved"
                    });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::Slider::new(&mut self.zoom, 50.0..=200.0).suffix("%"));
                });
            });
        });
    }
}

impl eframe::App for RowlyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_grid_keys(ctx);
        self.show_menu(ctx);
        self.show_toolbar(ctx);
        self.show_status(ctx);
        self.show_workspace(ctx);
    }
}

fn column_label(mut column: usize) -> String {
    column += 1;
    let mut letters = Vec::new();
    while column > 0 {
        let remainder = (column - 1) % 26;
        letters.push((b'A' + remainder as u8) as char);
        column = (column - 1) / 26;
    }
    letters.iter().rev().collect()
}

fn cell_range(first: CellRef, second: CellRef) -> (usize, usize, usize, usize) {
    (
        first.row().min(second.row()),
        first.row().max(second.row()),
        first.column().min(second.column()),
        first.column().max(second.column()),
    )
}

fn update_selection(
    selection: &mut CellRef,
    anchor: &mut CellRef,
    reference_input: &mut String,
    editing: &mut bool,
    reference: CellRef,
    extend: bool,
    bounds: (usize, usize),
) {
    let reference = CellRef::new(
        reference.row().min(bounds.0.saturating_sub(1)),
        reference.column().min(bounds.1.saturating_sub(1)),
    );
    if !extend {
        *anchor = reference;
    }
    *selection = reference;
    *reference_input = reference.to_string();
    *editing = false;
}

impl RowlyApp {
    fn handle_grid_keys(&mut self, ctx: &egui::Context) {
        if self.mode != WorkspaceMode::TableEditor || ctx.wants_keyboard_input() {
            return;
        }
        if self.editing {
            return;
        }

        let (undo, redo, edit, cancel, direction, extend) = ctx.input(|input| {
            let command = input.modifiers.command;
            let direction = if input.key_pressed(egui::Key::ArrowUp) {
                Some((0isize, -1isize))
            } else if input.key_pressed(egui::Key::ArrowDown) {
                Some((0, 1))
            } else if input.key_pressed(egui::Key::ArrowLeft) {
                Some((-1, 0))
            } else if input.key_pressed(egui::Key::ArrowRight) {
                Some((1, 0))
            } else {
                None
            };
            (
                command && input.key_pressed(egui::Key::Z) && !input.modifiers.shift,
                command
                    && (input.key_pressed(egui::Key::Y)
                        || input.modifiers.shift && input.key_pressed(egui::Key::Z)),
                input.key_pressed(egui::Key::F2) || input.key_pressed(egui::Key::Enter),
                input.key_pressed(egui::Key::Escape),
                direction,
                input.modifiers.shift,
            )
        });

        if undo {
            self.undo();
        } else if redo {
            self.redo();
        } else if cancel {
            self.editing = false;
        } else if let Some((dx, dy)) = direction {
            let next = CellRef::new(
                self.selection.row().saturating_add_signed(dy),
                self.selection.column().saturating_add_signed(dx),
            );
            self.set_selection(next, extend);
        } else if edit {
            let value = self
                .document
                .as_ref()
                .and_then(|document| document.cell(self.selection.row(), self.selection.column()))
                .map(str::to_owned);
            if let Some(value) = value {
                self.begin_edit(&value);
            } else {
                self.status = format!("{} is outside this CSV row", self.selection);
            }
        }
    }
}
