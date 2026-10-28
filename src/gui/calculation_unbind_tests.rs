use eframe::egui::{self, Event, Key, Modifiers};

use super::calculation_panel::CALCULATION_EXPRESSION_WIDGET_ID;
use super::command_tests::{edited_app, input_frame, press};
use super::{RowlyApp, WorkspaceMode};
use crate::gui_commands::{CommandFocus, CoreCommand};
use crate::process::{
    CellRef,
    calculation::{
        CalculationBindingId, CalculationExpression, CalculationTarget, CalculationTrigger,
        CalculationValue,
    },
};
use crate::project::{ProjectScripts, ProjectSource, RowlyProject, SourceKind};
use crate::project_session::ProjectSession;

// {
//   責務: [add_test_binding: 開いているCSVのA1へ手動計算規則を登録する。],
//   処理: [固定値を返す規則を計算エンジンへ追加する。],
//   引数: [app: CSVを開いた検証用画面。],
//   戻り値: [CalculationBindingId: 登録した規則の識別子。]
// }
fn add_test_binding(app: &mut RowlyApp) -> CalculationBindingId {
    let id = CalculationBindingId::new("dsl:A1").expect("有効な識別子");
    app.calculation_engine
        .set_binding(
            app.document.as_ref().expect("CSV fixture"),
            id.clone(),
            CalculationTarget::Cell(CellRef::new(0, 0)),
            CalculationExpression::Literal(CalculationValue::Integer(7)),
            CalculationTrigger::Manual,
        )
        .expect("規則を登録できる");
    id
}

// {
//   責務: [alt_unbind_removes_selected_binding_without_changing_csv: Alt経路の削除とCSV保持を検証する。],
//   処理: [Alt C Uを配送し、選択規則・dirty状態・CSV・履歴・diskを比較する。],
//   引数: [],
//   戻り値: [(): 計算規則だけが削除されれば成功する。]
// }
#[test]
fn alt_unbind_removes_selected_binding_without_changing_csv() {
    let (directory, mut app) = edited_app();
    let id = add_test_binding(&mut app);
    let ctx = egui::Context::default();
    input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
    let remaining = input_frame(
        &mut app,
        &ctx,
        Modifiers::NONE,
        vec![
            press(Key::C, Modifiers::NONE),
            Event::Text("c".to_owned()),
            press(Key::U, Modifiers::NONE),
            Event::Text("u".to_owned()),
        ],
    );
    assert!(remaining.is_empty());
    assert!(app.calculation_engine.binding(&id).is_none());
    assert!(app.calculation_bindings_dirty);
    let document = app.document.as_ref().expect("CSV fixture");
    assert_eq!(document.cell(0, 0), Some("new"));
    assert!(document.can_undo());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("table.csv")).expect("CSV fixture"),
        "old\n"
    );
}

// {
//   責務: [unbind_rejects_hidden_or_unrelated_selection: 対象が見えない状態や別の入力欄での削除を拒否する。],
//   処理: [別画面・inline編集・CSV不在・modalで要求し、規則とdirty状態を比較する。],
//   引数: [],
//   戻り値: [(): 無効な要求が計算規則を変更しなければ成功する。]
// }
#[test]
fn unbind_rejects_hidden_or_unrelated_selection() {
    let (_, mut app) = edited_app();
    let id = add_test_binding(&mut app);
    let ctx = egui::Context::default();
    for mode in [
        WorkspaceMode::TextEditor,
        WorkspaceMode::Viewer,
        WorkspaceMode::Help,
    ] {
        app.mode = mode;
        app.status = "要求前".to_owned();
        app.dispatch_core_command(&ctx, CoreCommand::RemoveCalculationBinding);
        assert!(app.status.contains("実行できません"));
        assert!(app.calculation_engine.binding(&id).is_some());
        assert!(!app.calculation_bindings_dirty);
    }
    app.mode = WorkspaceMode::TableEditor;
    app.begin_edit("未確定");
    app.status = "inline要求前".to_owned();
    app.dispatch_core_command(&ctx, CoreCommand::RemoveCalculationBinding);
    assert!(app.status.contains("実行できません"));
    assert!(app.calculation_engine.binding(&id).is_some());
    assert!(!app.calculation_bindings_dirty);
    assert_eq!(app.edit_value, "未確定");
    app.editing = false;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::Modal::new(egui::Id::new("unbind-modal")).show(ctx, |ui| {
            ui.label("modal");
        });
    });
    assert_eq!(app.command_context(&ctx).focus, CommandFocus::Modal);
    app.status = "modal要求前".to_owned();
    app.dispatch_core_command(&ctx, CoreCommand::RemoveCalculationBinding);
    assert!(app.status.contains("実行できません"));
    assert!(app.calculation_engine.binding(&id).is_some());
    assert!(!app.calculation_bindings_dirty);
    app.document = None;
    app.status = "CSVなし要求前".to_owned();
    app.dispatch_core_command(
        &egui::Context::default(),
        CoreCommand::RemoveCalculationBinding,
    );
    assert!(app.status.contains("実行できません"));
    assert!(app.calculation_engine.binding(&id).is_some());
    assert!(!app.calculation_bindings_dirty);
}

// {
//   責務: [unbind_shortcut_accepts_expression_focus: 計算式欄のfocus中に既存shortcutを実行する。],
//   処理: [TextEditを描画してfocusを設定し、Ctrl+Shift+Kで選択規則を削除する。],
//   引数: [],
//   戻り値: [(): 計算式のdraftを保持して規則を削除できれば成功する。]
// }
#[test]
fn unbind_shortcut_accepts_expression_focus() {
    let (_, mut app) = edited_app();
    let id = add_test_binding(&mut app);
    let ctx = egui::Context::default();
    let widget_id = egui::Id::new(CALCULATION_EXPRESSION_WIDGET_ID);
    let mut expression = "draft".to_owned();
    app.calculation_expression = "app draft".to_owned();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut expression).id(widget_id));
            ui.memory_mut(|memory| memory.request_focus(widget_id));
        });
    });
    // ShiftがないCtrl+Kは削除とは異なるため、入力イベントを保持する。
    let plain_command = Modifiers::COMMAND;
    app.status = "Ctrl+K要求前".to_owned();
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            modifiers: plain_command,
            events: vec![
                press(Key::K, plain_command),
                Event::Key {
                    key: Key::K,
                    physical_key: Some(Key::K),
                    pressed: false,
                    repeat: false,
                    modifiers: plain_command,
                },
            ],
            ..Default::default()
        },
        |ctx| {
            app.dispatch_core_shortcuts(ctx);
            assert!(!ctx.input(|input| input.events.is_empty()));
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add(egui::TextEdit::singleline(&mut expression).id(widget_id));
            });
        },
    );
    assert!(app.calculation_engine.binding(&id).is_some());
    assert!(!app.calculation_bindings_dirty);
    assert_eq!(app.status, "Ctrl+K要求前");
    let modifiers = Modifiers {
        shift: true,
        ..Modifiers::COMMAND
    };
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            modifiers,
            events: vec![press(Key::K, modifiers)],
            ..Default::default()
        },
        |ctx| {
            assert!(app.command_context(ctx).calculation_expression_focused);
            app.dispatch_core_shortcuts(ctx);
            assert!(ctx.input(|input| input.events.is_empty()));
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add(egui::TextEdit::singleline(&mut expression).id(widget_id));
            });
        },
    );
    assert!(app.calculation_engine.binding(&id).is_none());
    assert!(app.calculation_bindings_dirty);
    assert_eq!(app.calculation_expression, "app draft");
    assert_eq!(expression, "draft");
}

// {
//   責務: [unbind_shortcut_rejects_other_text_focus: 計算式以外の入力欄からのshortcutで規則を削除しない。],
//   処理: [別のTextEditにfocusを与え、Ctrl+Shift+Kを配送する。],
//   引数: [],
//   戻り値: [(): 規則と入力内容を保持すれば成功する。]
// }
#[test]
fn unbind_shortcut_rejects_other_text_focus() {
    let (_, mut app) = edited_app();
    let id = add_test_binding(&mut app);
    let ctx = egui::Context::default();
    let widget_id = egui::Id::new("unrelated-text-input");
    let mut text = "draft".to_owned();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut text).id(widget_id));
            ui.memory_mut(|memory| memory.request_focus(widget_id));
        });
    });
    let modifiers = Modifiers {
        shift: true,
        ..Modifiers::COMMAND
    };
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            modifiers,
            events: vec![press(Key::K, modifiers)],
            ..Default::default()
        },
        |ctx| {
            assert_eq!(app.command_context(ctx).focus, CommandFocus::TextInput);
            assert!(!app.command_context(ctx).calculation_expression_focused);
            app.dispatch_core_shortcuts(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add(egui::TextEdit::singleline(&mut text).id(widget_id));
            });
        },
    );
    assert!(app.calculation_engine.binding(&id).is_some());
    assert!(!app.calculation_bindings_dirty);
    assert_eq!(text, "draft");
}

// {
//   責務: [unbind_preserves_user_defined_binding: user.rlyが所有する計算規則の削除を拒否する。],
//   処理: [手書き規則を含むprojectを開き、選択セルの削除をRegistryから要求する。],
//   引数: [],
//   戻り値: [(): 規則・user.rly・CSVが保持されることを確認する。]
// }
#[test]
fn unbind_preserves_user_defined_binding() {
    use std::{fs, path::PathBuf};
    let (directory, mut app) = edited_app();
    let root = directory.path();
    fs::create_dir_all(root.join("scripts/macros")).expect("scripts fixture");
    fs::create_dir_all(root.join("history")).expect("history fixture");
    fs::write(
        root.join("scripts/init.rly"),
        "INCLUDE GENERATED\nINCLUDE USER\n",
    )
    .expect("init fixture");
    fs::write(root.join("scripts/generated.rly"), "' generated\n").expect("generated fixture");
    let user_script = "SET This.Project.Source(\"data\").Worksheet.Editor.Cell(\"A1\").Calculation.Expression = Integer(7)\n";
    fs::write(root.join("scripts/user.rly"), user_script).expect("user fixture");
    let project = RowlyProject {
        name: "Unbind guard test".into(),
        sources: vec![ProjectSource {
            id: "data".into(),
            kind: SourceKind::File,
            path: PathBuf::from("table.csv"),
            recursive: false,
            search_root: None,
            schema: None,
        }],
        scripts: ProjectScripts {
            init: PathBuf::from("scripts/init.rly"),
            generated: PathBuf::from("scripts/generated.rly"),
            user: PathBuf::from("scripts/user.rly"),
            macros: PathBuf::from("scripts/macros"),
        },
        history: PathBuf::from("history"),
    };
    let manifest = root.join("project.rwprj");
    project.save(&manifest).expect("manifest fixture");
    app.project_session = Some(ProjectSession::open(&manifest).expect("project fixture"));
    app.project_source_id = Some("data".to_owned());
    let id = add_test_binding(&mut app);
    app.dispatch_core_command(
        &egui::Context::default(),
        CoreCommand::RemoveCalculationBinding,
    );
    assert!(app.status.contains("user.rly"));
    assert!(app.calculation_engine.binding(&id).is_some());
    assert!(!app.calculation_bindings_dirty);
    // A1の手書き規則とB1のGUI規則が同じ選択範囲にある場合も、全体を拒否する。
    app.document
        .as_mut()
        .expect("CSV fixture")
        .insert_columns(1, 1)
        .expect("B1 fixture");
    let gui_id = CalculationBindingId::new("dsl:B1").expect("有効な識別子");
    app.calculation_engine
        .set_binding(
            app.document.as_ref().expect("CSV fixture"),
            gui_id.clone(),
            CalculationTarget::Cell(CellRef::new(0, 1)),
            CalculationExpression::Literal(CalculationValue::Integer(9)),
            CalculationTrigger::Manual,
        )
        .expect("B1 binding fixture");
    app.selection_anchor = CellRef::new(0, 0);
    app.selection = CellRef::new(0, 1);
    app.status = "範囲要求前".to_owned();
    app.dispatch_core_command(
        &egui::Context::default(),
        CoreCommand::RemoveCalculationBinding,
    );
    assert!(app.status.contains("user.rly"));
    assert!(app.calculation_engine.binding(&id).is_some());
    assert!(app.calculation_engine.binding(&gui_id).is_some());
    assert!(!app.calculation_bindings_dirty);
    assert_eq!(
        fs::read_to_string(root.join("scripts/user.rly")).expect("user fixture"),
        user_script
    );
    assert_eq!(
        fs::read_to_string(root.join("table.csv")).expect("CSV fixture"),
        "old\n"
    );
}
