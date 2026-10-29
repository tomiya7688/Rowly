use eframe::egui::{self, Event, Key, Modifiers};

use crate::gui_commands::CoreCommand;

use super::command_tests::{edited_app, input_frame, press};
use super::{HelpDocument, HelpTarget, RowlyApp, WorkspaceMode};

// {
//   責務: [calculation_raw_csv_button_switches_via_registry: 計算欄のボタンによるText Editorへの切替を検証する。],
//   処理: [実際の計算欄を描画してボタンをクリックし、CSVテキストと未適用の編集を比較する。],
//   引数: [],
//   戻り値: [(): 表示切替と編集内容の保持が正しければ成功する。]
// }
#[test]
fn calculation_raw_csv_button_switches_via_registry() {
    for dirty in [false, true] {
        let (directory, mut app) = edited_app();
        let ctx = egui::Context::default();
        app.code_buffer = "draft\n".to_owned();
        app.code_buffer_dirty = dirty;
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.show_calculation_panel(ui));
        });
        let position = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Open raw CSV in Code" => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("計算欄のボタンを描画する");
        for pressed in [true, false] {
            let _ = ctx.run(
                egui::RawInput {
                    focused: true,
                    events: vec![
                        Event::PointerMoved(position),
                        Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.show_calculation_panel(ui));
                },
            );
        }
        assert_eq!(app.mode, WorkspaceMode::TextEditor);
        assert_eq!(app.code_buffer, if dirty { "draft\n" } else { "new\n" });
        assert_eq!(app.code_buffer_dirty, dirty);
        assert_eq!(app.status, "Codeで A1 のraw CSV値を確認できます");
        assert_eq!(
            std::fs::read_to_string(directory.path().join("table.csv")).expect("CSV fixture"),
            "old\n"
        );
    }
}

// {
//   責務: [select_workspace_with_alt: 登録された画面切替のAlt経路を入力する。],
//   処理: [Altの単独押下後にVと指定したleafを送り、選択文字の消費を確認する。],
//   引数: [app: 切替対象のGUI。 ctx: 継続する入力状態。 leaf: V階層の画面選択キー。],
//   戻り値: [(): 共通dispatchを通して画面切替を要求する。]
// }
fn select_workspace_with_alt(app: &mut RowlyApp, ctx: &egui::Context, leaf: Key) {
    input_frame(app, ctx, Modifiers::ALT, vec![]);
    let remaining = input_frame(
        app,
        ctx,
        Modifiers::NONE,
        vec![
            press(Key::V, Modifiers::NONE),
            Event::Text("v".to_owned()),
            press(leaf, Modifiers::NONE),
            Event::Text(leaf.name().to_ascii_lowercase()),
        ],
    );
    assert!(remaining.is_empty(), "画面選択文字は編集欄へ渡さない");
}

// {
//   責務: [alt_workspace_routes_preserve_csv_and_history: 全画面間のAlt切替がCSVを変更しないことを検証する。],
//   処理: [Helpを含む各画面から3つの作業画面へ移り、表示先・CSV・履歴・diskを比較する。],
//   引数: [],
//   戻り値: [(): 全遷移のassertionを完了する。]
// }
#[test]
fn alt_workspace_routes_preserve_csv_and_history() {
    let (directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    for origin in [
        WorkspaceMode::TableEditor,
        WorkspaceMode::TextEditor,
        WorkspaceMode::Viewer,
        WorkspaceMode::Help,
    ] {
        for (leaf, target) in [
            (Key::E, WorkspaceMode::TableEditor),
            (Key::C, WorkspaceMode::TextEditor),
            (Key::V, WorkspaceMode::Viewer),
        ] {
            app.mode = origin;
            select_workspace_with_alt(&mut app, &ctx, leaf);
            assert_eq!(app.mode, target);
            let document = app.document.as_ref().expect("fixtureにCSVがある");
            assert_eq!(document.cell(0, 0), Some("new"));
            assert!(document.can_undo());
            assert!(!document.can_redo());
        }
    }
    assert_eq!(
        std::fs::read_to_string(directory.path().join("table.csv"))
            .expect("fixtureのCSVを読み取れる"),
        "old\n"
    );
}

// {
//   責務: [workspace_switch_refreshes_clean_text_and_preserves_drafts: テキスト画面への遷移と再選択のbuffer契約を検証する。],
//   処理: [正本からの同期後に未適用テキストを残して往復し、保存拒否とinline編集値の保持を確認する。],
//   引数: [],
//   戻り値: [(): 未適用編集を暗黙に適用・破棄しないことを確認する。]
// }
#[test]
fn workspace_switch_refreshes_clean_text_and_preserves_drafts() {
    let (directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    app.code_buffer = "stale\n".to_owned();
    app.dispatch_core_command(&ctx, CoreCommand::ShowTextEditor);
    assert_eq!(app.code_buffer, "new\n");
    // 画面の再選択を、正本からの再同期として扱わない。
    app.code_buffer = "draft\n".to_owned();
    app.dispatch_core_command(&ctx, CoreCommand::ShowTextEditor);
    assert_eq!(app.code_buffer, "draft\n");
    app.code_buffer_dirty = true;
    app.begin_edit("pending inline text");
    for (leaf, target) in [
        (Key::V, WorkspaceMode::Viewer),
        (Key::E, WorkspaceMode::TableEditor),
        (Key::C, WorkspaceMode::TextEditor),
    ] {
        select_workspace_with_alt(&mut app, &ctx, leaf);
        assert_eq!(app.mode, target);
        assert_eq!(app.code_buffer, "draft\n");
        assert!(app.code_buffer_dirty);
        assert!(app.editing);
        assert_eq!(app.edit_value, "pending inline text");
        app.dispatch_core_command(&ctx, CoreCommand::Save);
        assert!(app.status.contains("実行できません"));
    }
    assert_eq!(
        app.document
            .as_ref()
            .expect("fixtureにCSVがある")
            .cell(0, 0),
        Some("new")
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("table.csv"))
            .expect("fixtureのCSVを読み取れる"),
        "old\n"
    );
}

// {
//   責務: [workspace_commands_allow_empty_documents_and_restore_help_return_mode: CSV未読込時とHelpからの切替を検証する。],
//   処理: [空のGUIをAltで切り替え、Helpを再度開いたときの復帰先を確認する。],
//   引数: [],
//   戻り値: [(): document不在で画面切替が可能であることを確認する。]
// }
#[test]
fn workspace_commands_allow_empty_documents_and_restore_help_return_mode() {
    let mut app = RowlyApp::default();
    let ctx = egui::Context::default();
    app.code_buffer = "stale".to_owned();
    select_workspace_with_alt(&mut app, &ctx, Key::C);
    assert!(app.code_buffer.is_empty());
    app.open_help_target(HelpTarget::new(HelpDocument::Reference, "source"));
    select_workspace_with_alt(&mut app, &ctx, Key::V);
    assert_eq!(app.mode, WorkspaceMode::Viewer);
    app.open_help_target(HelpTarget::new(HelpDocument::Reference, "source"));
    assert_eq!(app.help_return_mode, WorkspaceMode::Viewer);
    select_workspace_with_alt(&mut app, &ctx, Key::E);
    assert_eq!(app.mode, WorkspaceMode::TableEditor);
    assert!(app.document.is_none());
}

// {
//   責務: [modal_blocks_all_workspace_commands: modal中の画面切替を全入口で拒否することを検証する。],
//   処理: [実egui modalを表示し、共通dispatchとAltから要求してもmodeとbufferが変わらないことを確認する。],
//   引数: [],
//   戻り値: [(): modalの操作対象を背後の画面切替へ渡さないことを確認する。]
// }
#[test]
fn modal_blocks_all_workspace_commands() {
    let (_directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::Modal::new(egui::Id::new("workspace-modal")).show(ctx, |ui| {
            ui.label("Modal fixture");
        });
    });
    for command in [
        CoreCommand::ShowTableEditor,
        CoreCommand::ShowTextEditor,
        CoreCommand::ShowViewer,
    ] {
        app.dispatch_core_command(&ctx, command);
        assert!(app.status.contains("実行できません"));
        assert_eq!(app.mode, WorkspaceMode::TableEditor);
        assert!(app.code_buffer.is_empty());
    }
    // 次frameでもmodalを表示し、Altからの背後操作を遮断する。
    let _ = ctx.run(
        egui::RawInput {
            modifiers: Modifiers::ALT,
            focused: true,
            ..Default::default()
        },
        |ctx| {
            egui::Modal::new(egui::Id::new("workspace-modal")).show(ctx, |ui| {
                ui.label("Modal fixture");
            });
            app.dispatch_keytips(ctx);
        },
    );
    let _ = ctx.run(
        egui::RawInput {
            events: vec![
                press(Key::V, Modifiers::NONE),
                press(Key::C, Modifiers::NONE),
            ],
            focused: true,
            ..Default::default()
        },
        |ctx| {
            egui::Modal::new(egui::Id::new("workspace-modal")).show(ctx, |ui| {
                ui.label("Modal fixture");
            });
            app.dispatch_keytips(ctx);
        },
    );
    assert_eq!(app.mode, WorkspaceMode::TableEditor);
}
