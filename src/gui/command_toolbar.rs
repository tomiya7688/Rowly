use eframe::egui;

use crate::gui_commands::CoreCommand;

use super::{RowlyApp, WorkspaceMode};

impl RowlyApp {
    // {
    //   責務: [show_toolbar: path・cell reference・編集操作・workspace切替用toolbarを描画する。]
    //   処理: [基本4操作はRegistryで表示・実行し、構造編集とworkspace切替は既存経路へ渡す。]
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
        let mut apply_code_requested = false;
        let previous_mode = self.mode;
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("CSV");
                ui.add(
                    egui::TextEdit::singleline(&mut self.path_input)
                        .hint_text("CSV file path")
                        .desired_width(360.0),
                );
                for command in [CoreCommand::Open, CoreCommand::Save] {
                    if self.show_core_command_button(ui, command, false) {
                        command_requested = Some(command);
                    }
                }
                if self.mode == WorkspaceMode::TextEditor
                    && ui
                        .add_enabled(self.document.is_some(), egui::Button::new("Apply Code"))
                        .clicked()
                {
                    apply_code_requested = true;
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
                ui.selectable_value(&mut self.mode, WorkspaceMode::TableEditor, "Table Editor");
                ui.selectable_value(&mut self.mode, WorkspaceMode::TextEditor, "Text Editor");
                ui.selectable_value(&mut self.mode, WorkspaceMode::Viewer, "Viewer");
            });
        });
        if let Some(command) = command_requested {
            self.dispatch_core_command(ctx, command);
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
