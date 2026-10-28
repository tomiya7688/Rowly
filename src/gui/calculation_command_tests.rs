use eframe::egui::{self, Event, Key, KeyboardShortcut, Modifiers};

use crate::gui_commands::{
    CommandContext, CommandDispatchError, CommandFocus, CommandMode, CoreCommand,
};
use crate::process::{
    CellRef,
    calculation::{
        CalculationBindingId, CalculationExpression, CalculationStatus, CalculationTarget,
        CalculationTrigger, CalculationValue,
    },
};

use super::command_tests::{edited_app, input_frame, press};
use super::{CalculationCommand, RowlyApp, WorkspaceMode};

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
                    calculation_expression_focused: false,
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
            app.dispatch_core_shortcuts(&ctx);
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

// {
//   責務: [calculation_recalculate_routes_preserve_drafts: 再計算のAltとF9がCSVと未適用編集を保持することを検証する。],
//   処理: [各画面からAlt C RとF9を送り、共通handlerの結果と入力消費を確認する。],
//   引数: [],
//   戻り値: [(): 再計算がCSV正本・未適用テキスト・編集中のセルを変えないことを確認する。]
// }
#[test]
fn calculation_recalculate_routes_preserve_drafts() {
    let (directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    let binding_id = CalculationBindingId::new("manual-test").expect("有効なbinding ID");
    app.code_buffer = "unapplied\n".to_owned();
    app.code_buffer_dirty = true;
    app.begin_edit("pending cell edit");
    for (index, mode) in [
        WorkspaceMode::TableEditor,
        WorkspaceMode::TextEditor,
        WorkspaceMode::Viewer,
        WorkspaceMode::Help,
    ]
    .into_iter()
    .enumerate()
    {
        app.mode = mode;
        app.status = "Alt要求前".to_owned();
        let expected = (index as i64 * 2 + 1).to_string();
        set_manual_test_binding(&mut app, &binding_id, index as i64 * 2 + 1);
        input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
        let remaining = input_frame(
            &mut app,
            &ctx,
            Modifiers::NONE,
            vec![
                press(Key::C, Modifiers::NONE),
                Event::Text("c".to_owned()),
                press(Key::R, Modifiers::NONE),
                Event::Text("r".to_owned()),
            ],
        );
        assert!(remaining.is_empty());
        assert_eq!(app.status, "計算結果を更新しました");
        assert_eq!(
            app.calculation_engine
                .result(&binding_id)
                .and_then(|result| result.value.as_deref()),
            Some(expected.as_str())
        );
        let expected_f9 = (index as i64 * 2 + 2).to_string();
        set_manual_test_binding(&mut app, &binding_id, index as i64 * 2 + 2);
        app.status = "before F9".to_owned();
        assert!(
            input_frame(
                &mut app,
                &ctx,
                Modifiers::NONE,
                vec![press(Key::F9, Modifiers::NONE)]
            )
            .is_empty()
        );
        assert_eq!(app.status, "計算結果を更新しました");
        assert_eq!(
            app.calculation_engine
                .result(&binding_id)
                .and_then(|result| result.value.as_deref()),
            Some(expected_f9.as_str())
        );
        app.status = "consumed".to_owned();
        app.dispatch_core_shortcuts(&ctx);
        assert_eq!(app.status, "consumed");
        assert_eq!(app.code_buffer, "unapplied\n");
        assert!(app.code_buffer_dirty);
        assert_eq!(app.edit_value, "pending cell edit");
        let document = app.document.as_ref().expect("fixtureにCSVがある");
        assert_eq!(document.cell(0, 0), Some("new"));
        assert!(document.can_undo());
    }
    assert_eq!(
        std::fs::read_to_string(directory.path().join("table.csv")).expect("CSV fixture"),
        "old\n"
    );
}

// {
//   責務: [set_manual_test_binding: 再計算前に手動bindingの結果を未評価へ戻す。],
//   処理: [fixtureのA1に固定値の手動計算規則を登録する。],
//   引数: [app: CSVを開いた検証用GUI。 binding_id: 規則のID。 integer: 評価する固定値。],
//   戻り値: [(): 登録に成功し、結果がPendingになる。]
// }
fn set_manual_test_binding(app: &mut RowlyApp, binding_id: &CalculationBindingId, integer: i64) {
    app.calculation_engine
        .set_binding(
            app.document.as_ref().expect("fixtureにCSVがある"),
            binding_id.clone(),
            CalculationTarget::Cell(CellRef::new(0, 0)),
            CalculationExpression::Literal(CalculationValue::Integer(integer)),
            CalculationTrigger::Manual,
        )
        .expect("手動計算のfixtureを登録できる");
    assert_eq!(
        app.calculation_engine
            .result(binding_id)
            .expect("未評価の結果がある")
            .status,
        CalculationStatus::Pending
    );
}

// {
//   責務: [calculation_recalculate_rejects_modal_and_missing_csv: 再計算の実行条件を検証する。],
//   処理: [modal表示中とCSV不在の状態でRegistryから再計算を要求する。],
//   引数: [],
//   戻り値: [(): 無効な要求が拒否され、編集中のテキストを保持することを確認する。]
// }
#[test]
fn calculation_recalculate_rejects_modal_and_missing_csv() {
    let (_, mut app) = edited_app();
    let ctx = egui::Context::default();
    app.code_buffer = "draft\n".to_owned();
    app.code_buffer_dirty = true;
    let binding_id = CalculationBindingId::new("manual-reject").expect("有効なbinding ID");
    set_manual_test_binding(&mut app, &binding_id, 1);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::Modal::new(egui::Id::new("recalculate-modal")).show(ctx, |ui| {
            ui.label("Modal fixture");
        });
    });
    assert_eq!(app.command_context(&ctx).focus, CommandFocus::Modal);
    app.status = "modal要求前".to_owned();
    app.dispatch_core_command(&ctx, CoreCommand::Recalculate);
    assert!(app.status.contains("実行できません"));
    assert_eq!(
        app.calculation_engine
            .result(&binding_id)
            .expect("結果がある")
            .status,
        CalculationStatus::Pending
    );
    app.document = None;
    app.status = "CSVなしの要求前".to_owned();
    app.dispatch_core_command(&egui::Context::default(), CoreCommand::Recalculate);
    assert!(app.status.contains("実行できません"));
    assert_eq!(
        app.calculation_engine
            .result(&binding_id)
            .expect("結果がある")
            .status,
        CalculationStatus::Pending
    );
    assert_eq!(app.code_buffer, "draft\n");
    assert!(app.code_buffer_dirty);
    assert_eq!(
        CoreCommand::Recalculate.id(),
        CalculationCommand::Recalculate.id()
    );
}

// {
//   責務: [calculation_recalculate_works_with_focused_text_widget: テキスト欄のfocus中にF9が再計算されることを検証する。],
//   処理: [eguiのTextEditを描画してfocusを渡し、入力frameからF9を配送する。],
//   引数: [],
//   戻り値: [(): テキスト入力中の再計算と入力欄の保持を確認する。]
// }
#[test]
fn calculation_recalculate_works_with_focused_text_widget() {
    let (_, mut app) = edited_app();
    let ctx = egui::Context::default();
    let widget_id = egui::Id::new("recalculate-text-focus");
    let mut text = "draft".to_owned();
    let binding_id = CalculationBindingId::new("manual-focus").expect("有効なbinding ID");
    set_manual_test_binding(&mut app, &binding_id, 7);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut text).id(widget_id));
            ui.memory_mut(|memory| memory.request_focus(widget_id));
        });
    });
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            events: vec![press(Key::F9, Modifiers::NONE)],
            ..Default::default()
        },
        |ctx| {
            assert_eq!(app.command_context(ctx).focus, CommandFocus::TextInput);
            app.dispatch_core_shortcuts(ctx);
            assert!(ctx.input(|input| input.events.is_empty()));
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = ui.add(egui::TextEdit::singleline(&mut text).id(widget_id));
                assert!(response.has_focus());
            });
        },
    );
    assert_eq!(app.status, "計算結果を更新しました");
    assert_eq!(
        app.calculation_engine
            .result(&binding_id)
            .and_then(|result| result.value.as_deref()),
        Some("7")
    );
    assert_eq!(text, "draft");
}
