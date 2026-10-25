use eframe::egui::{self, Event, Key, KeyboardShortcut, Modifiers};

use crate::gui_commands::{
    CommandContext, CommandDispatchError, CommandFocus, CommandMode, CoreCommand,
};

use super::command_tests::{edited_app, input_frame, press};
use super::{CalculationCommand, WorkspaceMode};

// {
//   責務: [calculation_show_metadata_covers_modes_and_focus: 計算欄表示のID・Alt経路・有効条件を検証する。],
//   処理: [全画面とfocus、CSV有無を組み合わせ、IDとshortcutで同じactionを取得する。],
//   引数: [],
//   戻り値: [(): modalまたはCSV不在ではhandlerが実行されないことを確認する。]
// }
#[test]
fn calculation_show_metadata_covers_modes_and_focus() {
    let (_, app) = edited_app();
    let registry = app.command_registry.as_ref().expect("基本操作を登録できる");
    let command = CoreCommand::ShowCalculationEditor;
    let definition = registry
        .commands()
        .find(|entry| entry.action == command)
        .expect("計算欄表示を登録している");
    assert_eq!(command.id(), CalculationCommand::Show.id());
    assert_eq!(definition.keytips_path, ["C", "S"]);
    let shortcut = KeyboardShortcut::new(
        Modifiers {
            shift: true,
            ..Modifiers::COMMAND
        },
        Key::F,
    );
    assert_eq!(definition.shortcuts, [shortcut]);
    for mode in [
        CommandMode::TableEditor,
        CommandMode::TextEditor,
        CommandMode::Viewer,
        CommandMode::Help,
    ] {
        for focus in [
            CommandFocus::Workspace,
            CommandFocus::TextInput,
            CommandFocus::Modal,
        ] {
            for has_document in [false, true] {
                let context = CommandContext {
                    mode,
                    focus,
                    has_document,
                    can_undo: false,
                    can_redo: false,
                    has_unapplied_csv_text: true,
                };
                let enabled = has_document && focus != CommandFocus::Modal;
                assert_eq!(definition.is_enabled(&context), enabled);
                let by_id = registry.dispatch(command.id(), &context, |action| *action);
                assert_eq!(by_id.is_ok(), enabled);
                let by_shortcut = registry.dispatch_shortcut(shortcut, &context, |action| *action);
                if enabled {
                    assert_eq!(by_id.expect("実行可能"), command);
                    assert_eq!(by_shortcut, Some(Ok(command)));
                } else if focus == CommandFocus::Modal {
                    assert_eq!(by_shortcut, None);
                } else {
                    assert_eq!(
                        by_shortcut,
                        Some(Err(CommandDispatchError::DisabledCommand(command.id())))
                    );
                }
            }
        }
    }
}

// {
//   責務: [calculation_show_alt_preserves_document_and_drafts: 全画面からのAlt経路が編集中の値を保持することを検証する。],
//   処理: [Alt C Sの選択文字を消費し、CSV・履歴・disk・テキスト・計算式を表示前後で比較する。],
//   引数: [],
//   戻り値: [(): 計算欄表示だけでは編集を確定・破棄・保存しないことを確認する。]
// }
#[test]
fn calculation_show_alt_preserves_document_and_drafts() {
    let (directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    app.code_buffer = "unapplied\n".to_owned();
    app.code_buffer_dirty = true;
    app.calculation_expression = "1 + 2".to_owned();
    app.begin_edit("pending cell edit");
    for origin in [
        WorkspaceMode::TableEditor,
        WorkspaceMode::TextEditor,
        WorkspaceMode::Viewer,
        WorkspaceMode::Help,
    ] {
        app.mode = origin;
        input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
        let remaining = input_frame(
            &mut app,
            &ctx,
            Modifiers::NONE,
            vec![
                press(Key::C, Modifiers::NONE),
                Event::Text("c".to_owned()),
                press(Key::S, Modifiers::NONE),
                Event::Text("s".to_owned()),
            ],
        );
        assert!(remaining.is_empty(), "Alt選択文字を編集欄へ渡さない");
        assert_eq!(app.mode, WorkspaceMode::TableEditor);
        assert_eq!(app.status, "Table EditorのCalculation欄を表示しました");
        assert_eq!(app.code_buffer, "unapplied\n");
        assert!(app.code_buffer_dirty);
        assert!(app.editing);
        assert_eq!(app.edit_value, "pending cell edit");
        assert_eq!(app.calculation_expression, "1 + 2");
        let document = app.document.as_ref().expect("fixtureにCSVがある");
        assert_eq!(document.cell(0, 0), Some("new"));
        assert!(document.can_undo());
        assert!(!document.can_redo());
    }
    assert_eq!(
        std::fs::read_to_string(directory.path().join("table.csv")).expect("fixtureのCSVを読む"),
        "old\n"
    );
}

// {
//   責務: [calculation_show_shortcut_uses_shared_dispatch: 計算欄表示のshortcutを全画面から実行できることを検証する。],
//   処理: [各画面でCtrl+Shift+Fを送り、消費後に旧計算shortcut処理が再実行しないことを確認する。],
//   引数: [],
//   戻り値: [(): テキスト入力状態でも表示し、入力イベントを二重配送しないことを確認する。]
// }
#[test]
fn calculation_show_shortcut_uses_shared_dispatch() {
    let (_, mut app) = edited_app();
    let ctx = egui::Context::default();
    let windows_modifiers = Modifiers {
        ctrl: true,
        command: true,
        shift: true,
        ..Modifiers::NONE
    };
    app.begin_edit("unfinished");
    // Windows相当の物理Ctrl併記と、論理primaryだけの両入力を確認する。
    for modifiers in [
        windows_modifiers,
        Modifiers {
            shift: true,
            ..Modifiers::COMMAND
        },
    ] {
        for origin in [
            WorkspaceMode::TableEditor,
            WorkspaceMode::TextEditor,
            WorkspaceMode::Viewer,
            WorkspaceMode::Help,
        ] {
            app.mode = origin;
            let remaining = input_frame(&mut app, &ctx, modifiers, vec![press(Key::F, modifiers)]);
            assert!(remaining.is_empty());
            assert_eq!(app.mode, WorkspaceMode::TableEditor);
            assert_eq!(app.status, "Table EditorのCalculation欄を表示しました");
            app.status = "consumed".to_owned();
            app.handle_calculation_shortcuts(&ctx);
            assert_eq!(app.status, "consumed");
            assert_eq!(app.edit_value, "unfinished");
        }
    }
}

// {
//   責務: [calculation_show_rejects_modal_and_missing_csv: 計算欄表示の実行時検査を検証する。],
//   処理: [CSV不在と描画済みmodalで表示要求を送り、画面と編集状態が変わらないことを確認する。],
//   引数: [],
//   戻り値: [(): 無効なcontextの操作が既存handlerへ到達しないことを確認する。]
// }
#[test]
fn calculation_show_rejects_modal_and_missing_csv() {
    let (_, mut app) = edited_app();
    app.mode = WorkspaceMode::TextEditor;
    app.code_buffer = "draft\n".to_owned();
    app.code_buffer_dirty = true;
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::Modal::new(egui::Id::new("calculation-show-modal")).show(ctx, |ui| {
            ui.label("Modal fixture");
        });
    });
    app.dispatch_core_command(&ctx, CoreCommand::ShowCalculationEditor);
    assert!(app.status.contains("実行できません"));
    assert_eq!(app.mode, WorkspaceMode::TextEditor);
    assert_eq!(app.code_buffer, "draft\n");
    assert!(app.code_buffer_dirty);
    app.document = None;
    app.status = "before missing CSV".to_owned();
    app.dispatch_core_command(
        &egui::Context::default(),
        CoreCommand::ShowCalculationEditor,
    );
    assert!(app.status.contains("実行できません"));
    assert_ne!(app.status, "before missing CSV");
    assert_eq!(app.mode, WorkspaceMode::TextEditor);
    assert_eq!(app.code_buffer, "draft\n");
}
