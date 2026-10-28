use eframe::egui;

use crate::{gui_commands::CoreCommand, project::SourceKind};

use super::{CalculationCommand, HelpDocument, HelpTarget, RowlyApp, WorkspaceMode};

impl RowlyApp {
    // {
    //   責務: [show_menu: ファイル・編集・計算・表示・ヘルプのmenuを描画し、選択操作を実行する。]
    //   処理: [ファイル・履歴・テキスト適用・画面切替・計算欄表示・再計算はRegistryから描画し、共通dispatchへ渡す。計算規則の登録と削除、ヘルプは既存handlerへ渡す。]
    //   引数: [self: 表示状態と操作対象document。 ctx: menu描画先。]
    //   戻り値: [(): menu表示と選択された操作を反映する。]
    // }
    pub(super) fn show_menu(&mut self, ctx: &egui::Context) {
        let mut command_requested = None;
        let mut calculation_apply_requested = false;
        let mut help_target = None;
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if self.show_core_command_button(ui, CoreCommand::Open, true) {
                        command_requested = Some(CoreCommand::Open);
                        ui.close_menu();
                    }
                    if self.show_core_command_button(ui, CoreCommand::Save, true) {
                        command_requested = Some(CoreCommand::Save);
                        ui.close_menu();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    for command in [
                        CoreCommand::Undo,
                        CoreCommand::Redo,
                        CoreCommand::ApplyCsvText,
                    ] {
                        if self.show_core_command_button(ui, command, true) {
                            command_requested = Some(command);
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("Formulas", |ui| {
                    let has_document = self.document.is_some();
                    if self.show_core_command_button(ui, CoreCommand::ShowCalculationEditor, true) {
                        command_requested = Some(CoreCommand::ShowCalculationEditor);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(has_document, egui::Button::new("Apply to selected cell"))
                        .clicked()
                    {
                        calculation_apply_requested = true;
                        ui.close_menu();
                    }
                    if self.show_core_command_button(ui, CoreCommand::Recalculate, true) {
                        command_requested = Some(CoreCommand::Recalculate);
                        ui.close_menu();
                    }
                    if self.show_core_command_button(
                        ui,
                        CoreCommand::RemoveCalculationBinding,
                        true,
                    ) {
                        command_requested = Some(CoreCommand::RemoveCalculationBinding);
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    for command in CoreCommand::WORKSPACE_SWITCHES {
                        if self.show_core_command_button(ui, command, false) {
                            command_requested = Some(command);
                            ui.close_menu();
                        }
                    }
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
        if let Some(command) = command_requested {
            self.dispatch_core_command(ctx, command);
        }
        if calculation_apply_requested {
            self.dispatch_calculation_command(CalculationCommand::Bind);
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
        let mut command_requested = None;
        let mut insert_row_requested = false;
        let mut delete_row_requested = false;
        let mut insert_column_requested = false;
        let mut delete_column_requested = false;
        let mut navigate_requested = false;
        let mut project_source_changed = false;
        let previous_project_source = self.project_source_id.clone();
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
                for command in [CoreCommand::Open, CoreCommand::Save] {
                    if self.show_core_command_button(ui, command, false) {
                        command_requested = Some(command);
                    }
                }
                if self.mode == WorkspaceMode::TextEditor
                    && self.show_core_command_button(ui, CoreCommand::ApplyCsvText, false)
                {
                    command_requested = Some(CoreCommand::ApplyCsvText);
                }
                for command in [CoreCommand::Undo, CoreCommand::Redo] {
                    if self.show_core_command_button(ui, command, false) {
                        command_requested = Some(command);
                    }
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
                for command in CoreCommand::WORKSPACE_SWITCHES {
                    if self.show_core_command_button(ui, command, false) {
                        command_requested = Some(command);
                    }
                }
            });
        });
        if let Some(command) = command_requested {
            self.dispatch_core_command(ctx, command);
        }
        if project_source_changed && self.project_source_id != previous_project_source {
            if let Some(source_id) = self.project_source_id.clone() {
                self.open_project_source(&source_id, previous_project_source.as_deref(), ctx);
            }
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
    }
}
