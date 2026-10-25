use eframe::egui::{self, Event, Key, Modifiers};

use crate::{
    gui_commands::{CommandFocus, CommandMode, CoreCommand},
    process::{ValidationRule, ValidationTarget},
};

use super::command_tests::{edited_app, input_frame, press};
use super::{RowlyApp, WorkspaceMode};

// {
//   責務: [alt_apply_csv_text_keeps_history_and_requires_explicit_save: Alt適用がCSV履歴を更新し、保存を分離することを検証する。],
//   処理: [Alt E Aで適用し、Undo/Redoと明示保存の前後でCSV正本とdiskを比較する。],
//   引数: [],
//   戻り値: [(): 選択文字の消費、履歴、保存のassertionを完了する。]
// }
#[test]
fn alt_apply_csv_text_keeps_history_and_requires_explicit_save() {
    let (directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    app.mode = WorkspaceMode::TextEditor;
    app.code_buffer = "applied\n".to_owned();
    app.code_buffer_dirty = true;
    input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
    let remaining = input_frame(
        &mut app,
        &ctx,
        Modifiers::NONE,
        vec![
            press(Key::E, Modifiers::NONE),
            Event::Text("e".to_owned()),
            press(Key::A, Modifiers::NONE),
            Event::Text("a".to_owned()),
        ],
    );
    assert!(remaining.is_empty(), "Alt選択文字をText Editorへ渡さない");
    assert!(!app.code_buffer_dirty);
    assert_eq!(app.code_buffer, "applied\n");
    assert_eq!(
        app.document
            .as_ref()
            .expect("fixtureにCSVがある")
            .cell(0, 0),
        Some("applied")
    );
    let path = directory.path().join("table.csv");
    assert_eq!(
        std::fs::read_to_string(&path).expect("fixtureのCSVを読み取れる"),
        "old\n"
    );
    // 適用はCSV全体の一つの編集として既存の履歴へ入る。
    app.dispatch_core_command(&ctx, CoreCommand::ShowTableEditor);
    app.dispatch_core_command(&ctx, CoreCommand::Undo);
    assert_eq!(
        app.document
            .as_ref()
            .expect("fixtureにCSVがある")
            .cell(0, 0),
        Some("new")
    );
    app.dispatch_core_command(&ctx, CoreCommand::Redo);
    assert_eq!(
        app.document
            .as_ref()
            .expect("fixtureにCSVがある")
            .cell(0, 0),
        Some("applied")
    );
    app.dispatch_core_command(&ctx, CoreCommand::Save);
    assert_eq!(
        std::fs::read_to_string(&path).expect("明示保存したCSVを読み取れる"),
        "applied\n"
    );
}

// {
//   責務: [apply_csv_text_metadata_restricts_mode_focus_and_document: テキスト適用のID・Alt経路・有効条件を検証する。],
//   処理: [全mode・focus・CSV有無の組合せを調べ、Text Editorのmodal外かつCSVありの場合だけ実行できることを確認する。],
//   引数: [],
//   戻り値: [(): Registry定義と実行条件のassertionを完了する。]
// }
#[test]
fn apply_csv_text_metadata_restricts_mode_focus_and_document() {
    let app = RowlyApp::default();
    let ctx = egui::Context::default();
    let registry = app
        .command_registry
        .as_ref()
        .expect("標準定義は衝突なく登録できる");
    let definition = registry
        .commands()
        .find(|entry| entry.action == CoreCommand::ApplyCsvText)
        .expect("追加操作を標準Registryへ登録する");
    assert_eq!(definition.id, "edit.apply_csv_text");
    assert_eq!(definition.keytips_path, ["E", "A"]);
    assert!(definition.shortcuts.is_empty());
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
                let mut context = app.command_context(&ctx);
                context.mode = mode;
                context.focus = focus;
                context.has_document = has_document;
                assert_eq!(
                    definition.is_enabled(&context),
                    mode == CommandMode::TextEditor && focus != CommandFocus::Modal && has_document
                );
            }
        }
    }
}

// {
//   責務: [apply_csv_text_rejections_preserve_document_and_draft: 無効な画面・文書不在・modalで適用しないことを検証する。],
//   処理: [共通dispatchから拒否条件で適用を要求し、CSV正本と未適用bufferを保持することを確認する。],
//   引数: [],
//   戻り値: [(): 無効操作がhandlerへ進まないことを確認する。]
// }
#[test]
fn apply_csv_text_rejections_preserve_document_and_draft() {
    let (_directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    app.code_buffer = "draft\n".to_owned();
    app.code_buffer_dirty = true;
    for mode in [
        WorkspaceMode::TableEditor,
        WorkspaceMode::Viewer,
        WorkspaceMode::Help,
    ] {
        app.mode = mode;
        app.dispatch_core_command(&ctx, CoreCommand::ApplyCsvText);
        assert!(app.status.contains("実行できません"));
        assert!(app.code_buffer_dirty);
        assert_eq!(app.code_buffer, "draft\n");
        assert_eq!(
            app.document
                .as_ref()
                .expect("fixtureにCSVがある")
                .cell(0, 0),
            Some("new")
        );
    }
    app.mode = WorkspaceMode::TextEditor;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::Modal::new(egui::Id::new("apply-text-modal")).show(ctx, |ui| {
            ui.label("Modal fixture");
        });
    });
    app.dispatch_core_command(&ctx, CoreCommand::ApplyCsvText);
    assert!(app.status.contains("実行できません"));
    assert!(app.code_buffer_dirty);
    assert_eq!(
        app.document
            .as_ref()
            .expect("fixtureにCSVがある")
            .cell(0, 0),
        Some("new")
    );
    let mut empty = RowlyApp {
        mode: WorkspaceMode::TextEditor,
        code_buffer: "draft\n".to_owned(),
        code_buffer_dirty: true,
        ..Default::default()
    };
    empty.dispatch_core_command(&egui::Context::default(), CoreCommand::ApplyCsvText);
    assert!(empty.status.contains("実行できません"));
    assert_eq!(empty.code_buffer, "draft\n");
    assert!(empty.code_buffer_dirty);
}

// {
//   責務: [validation_failure_keeps_csv_history_and_unapplied_text: 検証規則に反する適用の原子性を検証する。],
//   処理: [列の許可値に反するCSVテキストを適用し、正本・履歴・buffer・diskが失敗前の状態を保つことを確認する。],
//   引数: [],
//   戻り値: [(): 失敗を成功扱いせず、再編集用のテキストを保持することを確認する。]
// }
#[test]
fn validation_failure_keeps_csv_history_and_unapplied_text() {
    let (directory, mut app) = edited_app();
    let document = app.document.as_mut().expect("fixtureにCSVがある");
    // 先頭行は列名として扱われるため、検証規則の対象になるデータ行を用意する。
    document
        .apply_csv_text("value\nnew\n")
        .expect("fixtureの見出しとデータ行を作る");
    document
        .set_validation_rule(
            ValidationTarget::Index(0),
            ValidationRule::AllowedValues(vec!["old".into(), "new".into()]),
        )
        .expect("fixtureの既存値を許可する規則");
    let history = (document.can_undo(), document.can_redo());
    app.mode = WorkspaceMode::TextEditor;
    app.code_buffer = "value\nrejected\n".to_owned();
    app.code_buffer_dirty = true;
    let previous_status = app.status.clone();
    app.dispatch_core_command(&egui::Context::default(), CoreCommand::ApplyCsvText);
    let document = app.document.as_ref().expect("fixtureにCSVがある");
    assert_eq!(document.cell(1, 0), Some("new"));
    assert_eq!((document.can_undo(), document.can_redo()), history);
    assert_eq!(app.code_buffer, "value\nrejected\n");
    assert!(app.code_buffer_dirty);
    assert_ne!(app.status, previous_status);
    assert_eq!(
        std::fs::read_to_string(directory.path().join("table.csv"))
            .expect("fixtureのCSVを読み取れる"),
        "old\n"
    );
}
