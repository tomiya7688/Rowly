use eframe::egui;

use crate::gui_commands::CoreCommand;

use super::{RowlyApp, help};

impl RowlyApp {
    // {
    //   責務: [show_menu: file・edit・help menuを描画し、選択操作を実行する。]
    //   処理: [Registryのmetadataで4操作を描画し、選択した操作を共通dispatchへ渡す。]
    //   引数: [self: 表示状態と操作対象document。 ctx: menu描画先。]
    //   戻り値: [(): menu表示と選択された操作を反映する。]
    // }
    pub(super) fn show_menu(&mut self, ctx: &egui::Context) {
        let mut command_requested = None;
        let mut help_target = None;
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    for command in [CoreCommand::Open, CoreCommand::Save] {
                        if self.show_core_command_button(ui, command, true) {
                            command_requested = Some(command);
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("Edit", |ui| {
                    for command in [CoreCommand::Undo, CoreCommand::Redo] {
                        if self.show_core_command_button(ui, command, true) {
                            command_requested = Some(command);
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.label("Choose a workspace mode from the toolbar.");
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("DSL 文法リファレンス").clicked() {
                        help_target = Some(help::HelpTarget::new(
                            help::HelpDocument::Reference,
                            "source",
                        ));
                        ui.close_menu();
                    }
                    if ui.button("DSL チュートリアル").clicked() {
                        help_target = Some(help::HelpTarget::new(
                            help::HelpDocument::Tutorial,
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
        if let Some(target) = help_target {
            self.open_help_target(target);
        }
    }
}
