use eframe::egui::{self, Event, Key, Modifiers};

use super::WorkspaceMode;
use super::calculation_panel::CALCULATION_EXPRESSION_WIDGET_ID;
use super::command_tests::{edited_app, input_frame, press};
use crate::gui_commands::CoreCommand;
use crate::process::calculation::CalculationBindingId;

// {
//   責務: [bind_alt_and_shortcut_register_without_changing_csv: 登録のAlt経路とshortcutがCSVを保持することを検証する。],
//   処理: [Alt C BとCtrl+Enterで同じセルに式を登録し、結果とCSV履歴を確認する。],
//   引数: [],
//   戻り値: [(): 両経路が規則を登録しCSVを保持した場合に成功する。]
// }
#[test]
fn bind_alt_and_shortcut_register_without_changing_csv() {
    let (directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    let id = CalculationBindingId::new("dsl:A1").expect("有効な識別子");
    app.calculation_expression = "Integer(7)".to_owned();
    input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
    let remaining = input_frame(
        &mut app,
        &ctx,
        Modifiers::NONE,
        vec![
            press(Key::C, Modifiers::NONE),
            Event::Text("c".to_owned()),
            press(Key::B, Modifiers::NONE),
            Event::Text("b".to_owned()),
        ],
    );
    assert!(remaining.is_empty());
    assert!(app.calculation_engine.binding(&id).is_some());
    assert!(app.calculation_bindings_dirty);
    assert_eq!(
        app.calculation_engine
            .result(&id)
            .and_then(|result| result.value.as_deref()),
        Some("7")
    );
    app.calculation_expression = "Integer(8)".to_owned();
    app.status = "shortcut要求前".to_owned();
    let remaining = input_frame(
        &mut app,
        &ctx,
        Modifiers::COMMAND,
        vec![press(Key::Enter, Modifiers::COMMAND)],
    );
    assert!(remaining.is_empty());
    assert!(app.status.contains("計算規則を適用"));
    assert_eq!(
        app.calculation_engine
            .result(&id)
            .and_then(|r| r.value.as_deref()),
        Some("8")
    );
    let document = app.document.as_ref().expect("CSV fixture");
    assert_eq!(document.cell(0, 0), Some("new"));
    assert!(document.can_undo());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("table.csv")).expect("CSV fixture"),
        "old\n"
    );
}

// {
//   責務: [bind_rejects_hidden_selection: 別画面やinline編集中の登録を拒否する。],
//   処理: [画面と入力状態を変えてRegistry経由の登録を要求する。],
//   引数: [],
//   戻り値: [(): 拒否した要求が計算規則を変更しなければ成功する。]
// }
#[test]
fn bind_rejects_hidden_selection() {
    let (_directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    let id = CalculationBindingId::new("dsl:A1").expect("有効な識別子");
    app.calculation_expression = "Integer(7)".to_owned();
    for mode in [
        WorkspaceMode::TextEditor,
        WorkspaceMode::Viewer,
        WorkspaceMode::Help,
    ] {
        app.mode = mode;
        app.status = "要求前".to_owned();
        app.dispatch_core_command(&ctx, CoreCommand::BindCalculation);
        assert!(app.status.contains("実行できません"));
        assert!(app.calculation_engine.binding(&id).is_none());
    }
    app.mode = WorkspaceMode::TableEditor;
    app.begin_edit("unfinished");
    app.status = "inline要求前".to_owned();
    app.dispatch_core_command(&ctx, CoreCommand::BindCalculation);
    assert!(app.status.contains("実行できません"));
    assert_eq!(app.edit_value, "unfinished");
    assert!(app.calculation_engine.binding(&id).is_none());
    assert!(!app.calculation_bindings_dirty);
    app.editing = false;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::Modal::new(egui::Id::new("bind-modal")).show(ctx, |ui| {
            ui.label("modal");
        });
    });
    app.status = "modal要求前".to_owned();
    app.dispatch_core_command(&ctx, CoreCommand::BindCalculation);
    assert!(app.status.contains("実行できません"));
    assert!(app.calculation_engine.binding(&id).is_none());
    app.document = None;
    app.status = "CSVなし要求前".to_owned();
    app.dispatch_core_command(&egui::Context::default(), CoreCommand::BindCalculation);
    assert!(app.status.contains("実行できません"));
    assert!(app.calculation_engine.binding(&id).is_none());
}

// {
//   責務: [bind_shortcut_accepts_expression_focus: 計算式欄にfocusがある場合のCtrl+Enterを検証する。],
//   処理: [計算式欄のfocusを設定してshortcutを送り、登録結果とdraftを確認する。],
//   引数: [],
//   戻り値: [(): 計算式を保持して登録できれば成功する。]
// }
#[test]
fn bind_shortcut_accepts_expression_focus() {
    let (_directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    let widget_id = egui::Id::new(CALCULATION_EXPRESSION_WIDGET_ID);
    let mut expression = "Integer(9)".to_owned();
    app.calculation_expression = expression.clone();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut expression).id(widget_id));
            ui.memory_mut(|memory| memory.request_focus(widget_id));
        });
    });
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            modifiers: Modifiers::COMMAND,
            events: vec![press(Key::Enter, Modifiers::COMMAND)],
            ..Default::default()
        },
        |ctx| {
            assert!(app.command_context(ctx).calculation_expression_focused);
            app.dispatch_core_shortcuts(ctx);
            assert!(ctx.input(|input| input.events.is_empty()));
        },
    );
    let id = CalculationBindingId::new("dsl:A1").expect("有効な識別子");
    assert!(app.calculation_engine.binding(&id).is_some());
    assert_eq!(app.calculation_expression, "Integer(9)");
    assert!(app.calculation_bindings_dirty);
}

// {
//   責務: [bind_shortcut_dispatches_once_when_panel_is_drawn: Ctrl+Enterと計算欄のEnter処理が重複しないことを検証する。],
//   処理: [計算欄を描画しfocusを設定した後、Registryの実行直後にstatusを置いて再描画する。],
//   引数: [],
//   戻り値: [(): 計算欄の描画で二度目の登録が起きなければ成功する。]
// }
#[test]
fn bind_shortcut_dispatches_once_when_panel_is_drawn() {
    let (_directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    let widget_id = egui::Id::new(CALCULATION_EXPRESSION_WIDGET_ID);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            app.show_calculation_panel(ui);
            ui.memory_mut(|memory| memory.request_focus(widget_id));
        });
    });
    app.calculation_expression = "Integer(9)".to_owned();
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            modifiers: Modifiers::COMMAND,
            events: vec![press(Key::Enter, Modifiers::COMMAND)],
            ..Default::default()
        },
        |ctx| {
            app.dispatch_core_shortcuts(ctx);
            assert!(app.status.contains("計算規則を適用"));
            app.status = "登録は一回".to_owned();
            egui::CentralPanel::default().show(ctx, |ui| app.show_calculation_panel(ui));
        },
    );
    assert_eq!(app.status, "登録は一回");
}

// {
//   責務: [calculation_panel_does_not_process_command_enter: 計算欄がCtrl+Enterを独自に登録しないことを検証する。],
//   処理: [Registryを呼ばず計算欄へCtrl+Enterを配送し、登録が発生しないことを確認する。],
//   引数: [],
//   戻り値: [(): 計算欄がshortcutを重複処理しなければ成功する。]
// }
#[test]
fn calculation_panel_does_not_process_command_enter() {
    let (_directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    let widget_id = egui::Id::new(CALCULATION_EXPRESSION_WIDGET_ID);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            app.show_calculation_panel(ui);
            ui.memory_mut(|memory| memory.request_focus(widget_id));
        });
    });
    app.calculation_expression = "Integer(9)".to_owned();
    app.status = "入力前".to_owned();
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            modifiers: Modifiers::COMMAND,
            events: vec![press(Key::Enter, Modifiers::COMMAND)],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.show_calculation_panel(ui));
        },
    );
    assert_eq!(app.status, "入力前");
    assert!(!app.calculation_bindings_dirty);
}

// {
//   責務: [bind_shortcut_preserves_other_text_input: 他のテキスト欄のCtrl+Enterを登録操作が奪わないことを検証する。],
//   処理: [別のTextEditへfocusを設定してshortcutを配送し、入力と規則の状態を確認する。],
//   引数: [],
//   戻り値: [(): 入力を消費せず計算規則を変更しなければ成功する。]
// }
#[test]
fn bind_shortcut_preserves_other_text_input() {
    let (_directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    let widget_id = egui::Id::new("other-text-input");
    let mut text = "draft".to_owned();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut text).id(widget_id));
            ui.memory_mut(|memory| memory.request_focus(widget_id));
        });
    });
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            modifiers: Modifiers::COMMAND,
            events: vec![press(Key::Enter, Modifiers::COMMAND)],
            ..Default::default()
        },
        |ctx| {
            app.dispatch_core_shortcuts(ctx);
            assert!(!ctx.input(|input| input.events.is_empty()));
        },
    );
    let id = CalculationBindingId::new("dsl:A1").expect("有効な識別子");
    assert!(app.calculation_engine.binding(&id).is_none());
    assert!(!app.calculation_bindings_dirty);
    assert_eq!(text, "draft");
}
