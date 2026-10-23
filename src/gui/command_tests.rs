use eframe::egui::{self, Event, Key, Modifiers};

use super::{RowlyApp, WorkspaceMode};
use crate::{gui_commands::CoreCommand, process::CsvDocument};

// {
//   責務: [press: 押下時modifierを持つ非repeatのキーイベントを作る。],
//   処理: [logical keyとphysical keyを同じキーへ設定する。],
//   引数: [key: 押すキー。 modifiers: 押下時の修飾状態。],
//   戻り値: [Event: eguiへ渡す押下イベント。]
// }
fn press(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    }
}

// {
//   責務: [input_frame: OSのwindowを作らずGUI adapterへ入力frameを渡す。],
//   処理: [Alt操作・shortcut・gridの順に処理し、残った入力を呼出側へ返す。],
//   引数: [app: 検証するGUI。 ctx: frame間で保持するegui状態。 modifiers: frameの修飾状態。 events: 配送する入力。],
//   戻り値: [Vec<Event>: adapterが消費しなかった入力。]
// }
fn input_frame(
    app: &mut RowlyApp,
    ctx: &egui::Context,
    modifiers: Modifiers,
    events: Vec<Event>,
) -> Vec<Event> {
    // 一つのtapごとにreleaseを置く。同frameの二度目のEscを長押しと誤認させない。
    let events = events
        .into_iter()
        .flat_map(|event| {
            let release = match &event {
                Event::Key {
                    key,
                    physical_key,
                    modifiers,
                    pressed: true,
                    ..
                } => Some(Event::Key {
                    key: *key,
                    physical_key: *physical_key,
                    pressed: false,
                    repeat: false,
                    modifiers: *modifiers,
                }),
                _ => None,
            };
            std::iter::once(event).chain(release)
        })
        .collect();
    held_input_frame(app, ctx, modifiers, events)
}

// {
//   責務: [held_input_frame: releaseを補わずOS相当の押下・長押し状態をeguiへ渡す。],
//   処理: [入力frameでadapterを実行し、消費されなかったイベントを返す。],
//   引数: [app: 対象GUI, ctx: 継続するegui状態, modifiers: 修飾状態, events: 生の入力列。],
//   戻り値: [Vec<Event>: key release以外の未消費入力。]
// }
fn held_input_frame(
    app: &mut RowlyApp,
    ctx: &egui::Context,
    modifiers: Modifiers,
    events: Vec<Event>,
) -> Vec<Event> {
    let mut remaining = Vec::new();
    let _ = ctx.run(
        egui::RawInput {
            modifiers,
            events,
            focused: true,
            ..Default::default()
        },
        |ctx| {
            app.dispatch_keytips(ctx);
            app.dispatch_core_shortcuts(ctx);
            app.handle_grid_keys(ctx);
            app.show_keytips(ctx);
            remaining = ctx.input(|input| {
                input
                    .events
                    .iter()
                    .filter(|event| !matches!(event, Event::Key { pressed: false, .. }))
                    .cloned()
                    .collect()
            });
        },
    );
    remaining
}

// {
//   責務: [edited_app: 実CSVの未保存編集とUndo履歴を持つGUIを準備する。],
//   処理: [一時CSVを開き、process APIからA1を書き換える。],
//   引数: [],
//   戻り値: [(TempDir, RowlyApp): CSVの寿命を保持するdirectoryと画面状態。]
// }
fn edited_app() -> (tempfile::TempDir, RowlyApp) {
    let directory = tempfile::tempdir().expect("test directory");
    let path = directory.path().join("table.csv");
    std::fs::write(&path, "old\n").expect("CSV fixture");
    let mut document = CsvDocument::open(&path).expect("valid CSV fixture");
    document
        .set_cell_a1("A1", "new")
        .expect("valid fixture edit");
    (
        directory,
        RowlyApp {
            document: Some(document),
            path_input: path.to_string_lossy().into_owned(),
            ..Default::default()
        },
    )
}

// {
//   責務: [alt_history_and_save_share_process_history: Alt操作が実CSV履歴と保存へ接続されることを検証する。],
//   処理: [Alt E U・Alt E R・Alt F Sを別frameで配送し、値とdisk内容を確認する。],
//   引数: [],
//   戻り値: [(): Undo/Redoと明示保存のassertionを完了する。]
// }
#[test]
fn alt_history_and_save_share_process_history() {
    let (directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    for (category, key, expected) in [
        (Key::E, Key::U, "old"),
        (Key::E, Key::R, "new"),
        (Key::F, Key::S, "new"),
    ] {
        input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
        input_frame(
            &mut app,
            &ctx,
            Modifiers::NONE,
            vec![press(category, Modifiers::NONE)],
        );
        let remaining = input_frame(
            &mut app,
            &ctx,
            Modifiers::NONE,
            vec![
                press(key, Modifiers::NONE),
                Event::Text(key.name().to_owned()),
            ],
        );
        assert!(remaining.is_empty(), "KeyTips input must not reach widgets");
        assert_eq!(app.document.as_ref().unwrap().cell(0, 0), Some(expected));
    }
    assert_eq!(
        std::fs::read_to_string(directory.path().join("table.csv")).unwrap(),
        "new\n"
    );
}

// {
//   責務: [unapplied_text_blocks_save_in_every_mode: mode変更後も未適用textの保存拒否を確認する。],
//   処理: [全modeの共通dispatchとAlt経路から保存を要求し、diskを比較する。],
//   引数: [],
//   戻り値: [(): 未適用textを迂回して保存しないことを確認する。]
// }
#[test]
fn unapplied_text_blocks_save_in_every_mode() {
    let (directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    app.code_buffer_dirty = true;
    for mode in [
        WorkspaceMode::TableEditor,
        WorkspaceMode::TextEditor,
        WorkspaceMode::Viewer,
        WorkspaceMode::Help,
    ] {
        app.mode = mode;
        app.dispatch_core_command(&ctx, CoreCommand::Save);
        input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
        input_frame(
            &mut app,
            &ctx,
            Modifiers::NONE,
            vec![
                press(Key::F, Modifiers::NONE),
                press(Key::S, Modifiers::NONE),
            ],
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("table.csv")).unwrap(),
            "old\n"
        );
        assert!(app.document.as_ref().unwrap().can_undo());
    }
}

// {
//   責務: [inline_text_focus_keeps_undo_events_and_csv_values: inline編集時の履歴分離を検証する。],
//   処理: [Ctrl+ZとAlt E Uを配送し、CSV履歴が進まずCtrl+Zが入力欄用に残ることを確認する。],
//   引数: [],
//   戻り値: [(): 入力欄のUndoがCSVへ流れないことを確認する。]
// }
#[test]
fn inline_text_focus_keeps_undo_events_and_csv_values() {
    let (_directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    app.editing = true;
    let modifiers = Modifiers::CTRL | Modifiers::COMMAND;
    let remaining = input_frame(&mut app, &ctx, modifiers, vec![press(Key::Z, modifiers)]);
    assert_eq!(remaining.len(), 1);
    input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
    input_frame(
        &mut app,
        &ctx,
        Modifiers::NONE,
        vec![
            press(Key::E, Modifiers::NONE),
            press(Key::U, Modifiers::NONE),
        ],
    );
    assert_eq!(app.document.as_ref().unwrap().cell(0, 0), Some("new"));
}

// {
//   責務: [shortcut_redo_is_not_consumed_as_undo: Ctrl+Shift+Zの誤配送とrepeat実行を防ぐ。],
//   処理: [Undo後にRedo shortcutとrepeatを配送し、CSV値と入力消費を確認する。],
//   引数: [],
//   戻り値: [(): 履歴が一度だけ正しい向きへ進むことを確認する。]
// }
#[test]
fn shortcut_redo_is_not_consumed_as_undo() {
    let (_directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    app.dispatch_core_command(&ctx, CoreCommand::Undo);
    let modifiers = Modifiers::CTRL | Modifiers::COMMAND | Modifiers::SHIFT;
    assert!(held_input_frame(&mut app, &ctx, modifiers, vec![press(Key::Z, modifiers)]).is_empty());
    assert_eq!(app.document.as_ref().unwrap().cell(0, 0), Some("new"));
    let mut repeated = press(Key::Z, Modifiers::COMMAND);
    if let Event::Key { repeat, .. } = &mut repeated {
        *repeat = true;
    }
    held_input_frame(&mut app, &ctx, Modifiers::COMMAND, vec![repeated]);
    assert_eq!(app.document.as_ref().unwrap().cell(0, 0), Some("new"));
}

// {
//   責務: [escape_and_context_change_cancel_alt_selection: 取消とmode変更が途中経路を残さないことを検証する。],
//   処理: [ファイル階層からEscで戻り、rootで終了した後とmode変更後のS入力を調べる。],
//   引数: [],
//   戻り値: [(): CSVが保存されず通常入力が残ることを確認する。]
// }
#[test]
fn escape_and_context_change_cancel_alt_selection() {
    let (directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
    input_frame(
        &mut app,
        &ctx,
        Modifiers::NONE,
        vec![
            press(Key::F, Modifiers::NONE),
            press(Key::Escape, Modifiers::NONE),
            press(Key::Escape, Modifiers::NONE),
        ],
    );
    assert_eq!(
        input_frame(
            &mut app,
            &ctx,
            Modifiers::NONE,
            vec![press(Key::S, Modifiers::NONE)]
        )
        .len(),
        1
    );
    input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
    input_frame(
        &mut app,
        &ctx,
        Modifiers::NONE,
        vec![press(Key::F, Modifiers::NONE)],
    );
    app.mode = WorkspaceMode::Viewer;
    assert_eq!(
        input_frame(
            &mut app,
            &ctx,
            Modifiers::NONE,
            vec![press(Key::S, Modifiers::NONE)]
        )
        .len(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("table.csv")).unwrap(),
        "old\n"
    );
}

// {
//   責務: [text_widget_and_modal_do_not_execute_csv_undo: 実TextEditとmodal状態のadapter境界を検証する。],
//   処理: [focusのあるTextEditとmodal layerを作り、履歴操作がCSVへ流れないことを確認する。],
//   引数: [],
//   戻り値: [(): 入力欄用event保持とmodalでの拒否を確認する。]
// }
#[test]
fn text_widget_and_modal_do_not_execute_csv_undo() {
    let (_directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    let mut text = String::from("input");
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut text))
                .request_focus();
        });
    });
    assert_eq!(
        app.command_context(&ctx).focus,
        crate::gui_commands::CommandFocus::TextInput
    );
    let modifiers = Modifiers::CTRL | Modifiers::COMMAND;
    assert_eq!(
        input_frame(&mut app, &ctx, modifiers, vec![press(Key::Z, modifiers)]).len(),
        1
    );
    // modal layerは実frame内で登録し、end_pass後の次frameで遮蔽される状態を使う。
    let _ = ctx.run(Default::default(), |ctx| {
        egui::Modal::new(egui::Id::new("test_modal")).show(ctx, |ui| {
            ui.label("履歴を遮蔽する確認用dialog");
        });
    });
    let _ = ctx.run(Default::default(), |ctx| {
        assert_eq!(
            app.command_context(ctx).focus,
            crate::gui_commands::CommandFocus::Modal
        );
        app.dispatch_core_command(ctx, CoreCommand::Undo);
    });
    assert_eq!(app.document.as_ref().unwrap().cell(0, 0), Some("new"));
}

// {
//   責務: [alt_open_loads_csv_and_preserves_later_text: Alt F OがCSVを開き、終了後の別入力を保持する。],
//   処理: [未読込GUIへAlt F Oと後続Xを配送し、documentと未消費文字を確認する。],
//   引数: [],
//   戻り値: [(): Openの実行とKeyTips文字だけの消費を確認する。]
// }
#[test]
fn alt_open_loads_csv_and_preserves_later_text() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("open.csv");
    std::fs::write(&path, "loaded\n").expect("fixture CSV");
    let mut app = RowlyApp {
        path_input: path.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let ctx = egui::Context::default();
    input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
    let remaining = input_frame(
        &mut app,
        &ctx,
        Modifiers::NONE,
        vec![
            press(Key::F, Modifiers::NONE),
            Event::Text("f".to_owned()),
            press(Key::O, Modifiers::NONE),
            Event::Text("o".to_owned()),
            press(Key::X, Modifiers::NONE),
            Event::Text("x".to_owned()),
        ],
    );
    assert_eq!(app.document.as_ref().unwrap().cell(0, 0), Some("loaded"));
    assert!(
        remaining
            .iter()
            .any(|event| matches!(event, Event::Text(text) if text == "x"))
    );
    assert!(
        !remaining
            .iter()
            .any(|event| matches!(event, Event::Text(text) if text == "f" || text == "o"))
    );
}

// {
//   責務: [alt_repress_cancels_selection: Alt再押下が階層操作を終了することを検証する。],
//   処理: [ファイル階層でAltを再押下し、Sが通常入力として残りCSVが保存されないことを確認する。],
//   引数: [],
//   戻り値: [(): Altによる取消とdisk不変を確認する。]
// }
#[test]
fn alt_repress_cancels_selection() {
    let (directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
    input_frame(
        &mut app,
        &ctx,
        Modifiers::NONE,
        vec![press(Key::F, Modifiers::NONE)],
    );
    input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
    assert_eq!(
        input_frame(
            &mut app,
            &ctx,
            Modifiers::NONE,
            vec![press(Key::S, Modifiers::NONE)]
        )
        .len(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("table.csv")).unwrap(),
        "old\n"
    );
}

// {
//   責務: [alt_shift_and_ime_preserve_normal_input: 言語切替とIME入力をAlt階層から分離する。],
//   処理: [Alt+Shiftでは開始せず、操作中のIMEで終了して通常文字を保持することを確認する。],
//   引数: [],
//   戻り値: [(): 修飾キーと日本語入力の非消費を確認する。]
// }
#[test]
fn alt_shift_and_ime_preserve_normal_input() {
    let (_directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    input_frame(&mut app, &ctx, Modifiers::ALT | Modifiers::SHIFT, vec![]);
    let remaining = input_frame(
        &mut app,
        &ctx,
        Modifiers::NONE,
        vec![Event::Text("normal".to_owned())],
    );
    assert_eq!(remaining, vec![Event::Text("normal".to_owned())]);
    input_frame(&mut app, &ctx, Modifiers::ALT, vec![]);
    let events = vec![
        Event::Ime(egui::ImeEvent::Enabled),
        Event::Text("日本語".to_owned()),
    ];
    assert_eq!(
        input_frame(&mut app, &ctx, Modifiers::NONE, events.clone()),
        events
    );
    assert_eq!(
        input_frame(
            &mut app,
            &ctx,
            Modifiers::NONE,
            vec![press(Key::S, Modifiers::NONE)]
        )
        .len(),
        1
    );
}

// {
//   責務: [queued_shortcuts_recheck_history_after_each_action: 同frame内の履歴変化を後続shortcutへ反映する。],
//   処理: [Undoに続けてRedoを配送し、開始時にはRedo不可でも元の編集値へ戻ることを確認する。],
//   引数: [],
//   戻り値: [(): 初期enabled結果を後続操作に流用しないことを確認する。]
// }
#[test]
fn queued_shortcuts_recheck_history_after_each_action() {
    let (_directory, mut app) = edited_app();
    let ctx = egui::Context::default();
    let undo = Modifiers::CTRL | Modifiers::COMMAND;
    let redo = undo | Modifiers::SHIFT;
    assert!(
        input_frame(
            &mut app,
            &ctx,
            Modifiers::NONE,
            vec![press(Key::Z, undo), press(Key::Z, redo)]
        )
        .is_empty()
    );
    assert_eq!(app.document.as_ref().unwrap().cell(0, 0), Some("new"));
    assert!(app.document.as_ref().unwrap().can_undo());
    assert!(!app.document.as_ref().unwrap().can_redo());
}
