use eframe::egui;

use crate::gui_commands::{CommandContext, CommandFocus, CommandMode, CoreCommand};
use crate::process::CsvDocument;

use super::{RowlyApp, WorkspaceMode};

impl RowlyApp {
    // {
    //   責務: [command_context: GUI操作の実行直前にmode・focus・CSV状態を取得する。],
    //   処理: [modalを優先し、inline編集とTextEditのfocusをCSV履歴から分離する。],
    //   引数: [self: 現在の画面状態, ctx: focusとmodalを保持するegui context。],
    //   戻り値: [CommandContext: Registryの有効条件へ渡す現在値。]
    // }
    pub(super) fn command_context(&self, ctx: &egui::Context) -> CommandContext {
        let (modal_open, focused_widget) =
            ctx.memory(|memory| (memory.top_modal_layer().is_some(), memory.focused()));
        let text_focused = self.editing
            || focused_widget.is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some());
        CommandContext {
            mode: match self.mode {
                WorkspaceMode::TableEditor => CommandMode::TableEditor,
                WorkspaceMode::TextEditor => CommandMode::TextEditor,
                WorkspaceMode::Viewer => CommandMode::Viewer,
                WorkspaceMode::Help => CommandMode::Help,
            },
            focus: if modal_open {
                CommandFocus::Modal
            } else if text_focused {
                CommandFocus::TextInput
            } else {
                CommandFocus::Workspace
            },
            has_document: self.document.is_some(),
            can_undo: self.document.as_ref().is_some_and(CsvDocument::can_undo),
            can_redo: self.document.as_ref().is_some_and(CsvDocument::can_redo),
            has_unapplied_csv_text: self.code_buffer_dirty,
        }
    }

    // {
    //   責務: [show_core_command_button: Registryの名前・説明・有効条件で共通buttonを描画する。],
    //   処理: [登録定義からlabelとshortcutを取得し、現在contextでbuttonを有効化する。],
    //   引数: [self: Registryと現在状態, ui: 描画先, command: 表示する操作, show_shortcut: shortcutを併記するか。],
    //   戻り値: [bool: 有効なbuttonが選択された場合にtrue。]
    // }
    pub(super) fn show_core_command_button(
        &self,
        ui: &mut egui::Ui,
        command: CoreCommand,
        show_shortcut: bool,
    ) -> bool {
        let Ok(registry) = &self.command_registry else {
            return false;
        };
        let Some(definition) = registry.commands().find(|entry| entry.action == command) else {
            return false;
        };
        let mut button = egui::Button::new(definition.name);
        if let Some(shortcut) = definition.shortcuts.first().filter(|_| show_shortcut) {
            button = button.shortcut_text(ui.ctx().format_shortcut(shortcut));
        }
        ui.add_enabled(
            definition.is_enabled(&self.command_context(ui.ctx())),
            button,
        )
        .on_hover_text(definition.description)
        .clicked()
    }

    // {
    //   責務: [dispatch_core_command: menu・toolbar・shortcutからの要求を共通dispatchで検査する。],
    //   処理: [最新contextでstable IDを解決し、許可された操作だけを既存GUI handlerへ渡す。],
    //   引数: [self: Registryと操作対象, ctx: 現在のegui context, command: 要求された操作。],
    //   戻り値: [(): 実行結果または拒否理由をstatusへ反映する。],
    //   副作用: [許可されたOpen/Save/Undo/Redoをprocess API経由で実行する。]
    // }
    pub(super) fn dispatch_core_command(&mut self, ctx: &egui::Context, command: CoreCommand) {
        let action = match &self.command_registry {
            Ok(registry) => registry
                .dispatch(command.id(), &self.command_context(ctx), |action| *action)
                .map_err(|error| error.to_string()),
            Err(error) => Err(format!("GUI操作の登録に失敗しました: {error}")),
        };
        // Registryの参照を解放してから画面・documentを更新する。
        match action {
            Ok(CoreCommand::Open) => self.open_csv(ctx),
            Ok(CoreCommand::Save) => self.save_csv(),
            Ok(CoreCommand::Undo) => self.undo(),
            Ok(CoreCommand::Redo) => self.redo(),
            Err(error) => self.status = error,
        }
    }

    // {
    //   責務: [dispatch_core_shortcuts: 登録済みshortcutをwidgetへの配送前に解決する。],
    //   処理: [押下時のmodifierで登録定義を照合し、解決したイベントだけを消費して共通dispatchへ渡す。],
    //   引数: [self: Registryと操作対象, ctx: 現在frameの入力を保持するegui context。],
    //   戻り値: [(): 許可された操作を実行する。入力欄固有のUndo/Redoは消費しない。]
    // }
    pub(super) fn dispatch_core_shortcuts(&mut self, ctx: &egui::Context) {
        let Ok(registry) = &self.command_registry else {
            return;
        };
        let context = self.command_context(ctx);
        let mut commands = Vec::new();
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                let egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    repeat,
                    ..
                } = event
                else {
                    return true;
                };
                let shortcut = egui::KeyboardShortcut::new(*modifiers, *key);
                let result = registry.dispatch_shortcut(shortcut, &context, |action| *action);
                match result {
                    None => true,
                    Some(_) => {
                        // 長押しによる再openと、同じ押下のwidgetへの二重配送を避ける。
                        if !repeat {
                            commands.push(shortcut);
                        }
                        false
                    }
                }
            });
        });
        // 入力lockを保持したままI/Oやrepaintを呼ばない。複数要求も実行時に再検査する。
        for shortcut in commands {
            // 前のUndo/Openで変わった履歴とdocument状態を、候補ごとに取り直す。
            let result = match &self.command_registry {
                Ok(registry) => {
                    registry
                        .dispatch_shortcut(shortcut, &self.command_context(ctx), |action| *action)
                }
                Err(error) => {
                    self.status = format!("GUI操作の登録に失敗しました: {error}");
                    return;
                }
            };
            match result {
                Some(Ok(command)) => self.dispatch_core_command(ctx, command),
                Some(Err(error)) => self.status = error.to_string(),
                None => {}
            }
        }
    }
}
