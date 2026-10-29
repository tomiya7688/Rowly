use eframe::egui;

use crate::project::SourceKind;

use super::{CalculationCommand, CsvDocument, HelpDocument, HelpTarget, RowlyApp, WorkspaceMode};

impl RowlyApp {
    // {
    //   責務: [show_menu: file・edit・help menuを描画し、選択操作を実行する。]
    //   処理: [menu actionを記録してUI描画後にopen・save・undo・redo・help遷移を呼ぶ。]
    //   引数: [self: 表示状態と操作対象document。 ctx: menu描画先。]
    //   戻り値: [(): menu表示と選択された操作を反映する。]
    // }
    pub(super) fn show_menu(&mut self, ctx: &egui::Context) {
        let mut open_requested = false;
        let mut save_requested = false;
        let mut calculation_apply_requested = false;
        let mut calculation_recalculate_requested = false;
        let mut calculation_remove_requested = false;
        let mut calculation_show_requested = false;
        let mut help_target = None;
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
                ui.menu_button("Formulas", |ui| {
                    let has_document = self.document.is_some();
                    if ui
                        .add_enabled(has_document, egui::Button::new("Show Calculation Editor"))
                        .clicked()
                    {
                        calculation_show_requested = true;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(has_document, egui::Button::new("Apply to selected cell"))
                        .clicked()
                    {
                        calculation_apply_requested = true;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(has_document, egui::Button::new("Recalculate"))
                        .clicked()
                    {
                        calculation_recalculate_requested = true;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(has_document, egui::Button::new("Remove binding"))
                        .clicked()
                    {
                        calculation_remove_requested = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.label("Choose a workspace mode from the toolbar.");
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("DSL 文法リファレンス").clicked() {
                        help_target = Some(HelpTarget::new(HelpDocument::Reference, "source"));
                        ui.close_menu();
                    }
                    if ui.button("DSL チュートリアル").clicked() {
                        help_target = Some(HelpTarget::new(
                            HelpDocument::Tutorial,
                            "tutorial-getting-started",
                        ));
                        ui.close_menu();
                    }
                });
                ui.separator();
                ui.label("Rowly");
            });
        });
        if open_requested {
            self.open_csv(ctx);
        }
        if save_requested {
            self.save_csv();
        }
        if calculation_apply_requested {
            self.dispatch_calculation_command(CalculationCommand::Bind);
        }
        if calculation_recalculate_requested {
            self.dispatch_calculation_command(CalculationCommand::Recalculate);
        }
        if calculation_remove_requested {
            self.dispatch_calculation_command(CalculationCommand::Unbind);
        }
        if calculation_show_requested {
            self.dispatch_calculation_command(CalculationCommand::Show);
        }
        if let Some(target) = help_target {
            self.open_help_target(target);
        }
    }

    // {
    //   責務: [show_toolbar: path・cell reference・編集操作・workspace切替用toolbarを描画する。]
    //   処理: [document状態に応じた操作を有効化し、UI描画後に選択された操作を実行する。]
    //   引数: [self: 入力値・workspace・documentを保持する画面状態。 ctx: toolbar描画先。]
    //   戻り値: [(): toolbar表示とユーザー操作を反映する。]
    // }
    pub(super) fn show_toolbar(&mut self, ctx: &egui::Context) {
        if self.mode == WorkspaceMode::Help {
            return;
        }
        let mut open_requested = false;
        let mut save_requested = false;
        let mut undo_requested = false;
        let mut redo_requested = false;
        let mut insert_row_requested = false;
        let mut delete_row_requested = false;
        let mut insert_column_requested = false;
        let mut delete_column_requested = false;
        let mut navigate_requested = false;
        let mut apply_code_requested = false;
        let mut project_source_changed = false;
        let previous_project_source = self.project_source_id.clone();
        let previous_mode = self.mode;
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("CSV / Project");
                ui.add(
                    egui::TextEdit::singleline(&mut self.path_input)
                        .hint_text("CSV, .rwprj, or .rowlyx path")
                        .desired_width(360.0),
                );
                if let Some(session) = self.project_session.as_ref() {
                    egui::ComboBox::from_id_salt("project_source")
                        .selected_text(self.project_source_id.as_deref().unwrap_or("Source"))
                        .show_ui(ui, |ui| {
                            for source in session
                                .project()
                                .sources
                                .iter()
                                .filter(|source| source.kind == SourceKind::File)
                            {
                                let previous_id = self.project_source_id.clone();
                                ui.selectable_value(
                                    &mut self.project_source_id,
                                    Some(source.id.clone()),
                                    source.id.as_str(),
                                );
                                project_source_changed |= previous_id != self.project_source_id;
                            }
                        });
                }
                if ui.button("Open").clicked() {
                    open_requested = true;
                }
                if ui
                    .add_enabled(
                        self.document.is_some()
                            && !(self.mode == WorkspaceMode::TextEditor && self.code_buffer_dirty),
                        egui::Button::new("Save"),
                    )
                    .clicked()
                {
                    save_requested = true;
                }
                if self.mode == WorkspaceMode::TextEditor
                    && ui
                        .add_enabled(self.document.is_some(), egui::Button::new("Apply Code"))
                        .clicked()
                {
                    apply_code_requested = true;
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
            self.open_csv(ctx);
        }
        if project_source_changed && self.project_source_id != previous_project_source {
            if let Some(source_id) = self.project_source_id.clone() {
                self.open_project_source(&source_id, previous_project_source.as_deref(), ctx);
            }
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
        if (insert_row_requested
            || delete_row_requested
            || insert_column_requested
            || delete_column_requested)
            && let Some(document) = self.document.as_ref()
            && let Err(error) = self.calculation_engine.recalculate_all(document)
        {
            self.status = error.to_string();
        }
        if navigate_requested {
            self.navigate_to_cell();
        }
        if previous_mode != WorkspaceMode::TextEditor
            && self.mode == WorkspaceMode::TextEditor
            && !self.code_buffer_dirty
        {
            self.refresh_code_buffer();
        }
        if apply_code_requested {
            self.apply_code_buffer();
        }
    }
}
