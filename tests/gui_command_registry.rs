#![cfg(feature = "gui")]

use std::error::Error;

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use rowly::gui_commands::{
    CommandContext, CommandDispatchError, CommandFocus, CommandMode, CommandRegistry,
    CommandRegistryError, CommandScope, CoreCommand, core_command_definitions,
};

// {
//   責務: [editable_context: 保存・履歴操作が可能なTable Editor状態をfixtureとして返す。],
//   処理: [CSVあり、通常focus、Undo/Redo可、未適用textなしの状態を作る。],
//   引数: [],
//   戻り値: [CommandContext: 各testで条件を変えて使う基準状態。]
// }
fn editable_context() -> CommandContext {
    CommandContext {
        mode: CommandMode::TableEditor,
        focus: CommandFocus::Workspace,
        calculation_expression_focused: false,
        has_document: true,
        can_undo: true,
        can_redo: true,
        has_unapplied_csv_text: false,
    }
}

// {
//   責務: [ids_and_all_shortcut_aliases_use_the_same_action: 複数入口が同じactionを実行する契約を検証する。],
//   処理: [全基本操作をIDと各shortcutでdispatchし、結果と実行回数を比較する。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、または登録・dispatchエラー。]
// }
#[test]
fn ids_and_all_shortcut_aliases_use_the_same_action() -> Result<(), Box<dyn Error>> {
    let registry = CommandRegistry::new(core_command_definitions())?;
    assert_eq!(registry.commands().len(), 12);
    for definition in registry.commands() {
        // Registryは空scopeを拒否するため、各定義の先頭mode/focusを有効なfixtureに使える。
        let context = CommandContext {
            mode: definition.scope.modes[0],
            focus: definition.scope.focuses[0],
            ..editable_context()
        };
        assert_eq!(definition.id, definition.action.id());
        let mut calls = 0;
        let by_id = registry.dispatch(definition.id, &context, |action| {
            calls += 1;
            *action
        })?;
        for shortcut in &definition.shortcuts {
            assert_eq!(
                registry.dispatch_shortcut(*shortcut, &context, |action| {
                    calls += 1;
                    *action
                }),
                Some(Ok(by_id))
            );
        }
        assert_eq!(calls, definition.shortcuts.len() + 1);
    }
    Ok(())
}

// {
//   責務: [workspace_ids_and_alt_paths_cover_document_and_focus_states: 画面切替の固定ID・Alt経路・有効条件を検証する。],
//   処理: [Table Editor・Text Editor・Viewer・HelpでCSVの有無とfocusを組み合わせ、modal以外で画面切替が有効になることを確認する。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、またはRegistryの登録エラー。]
// }
#[test]
fn workspace_ids_and_alt_paths_cover_document_and_focus_states() -> Result<(), Box<dyn Error>> {
    let registry = CommandRegistry::new(core_command_definitions())?;
    for (command, id, path) in [
        (
            CoreCommand::ShowTableEditor,
            "view.table_editor",
            ["V", "E"],
        ),
        (CoreCommand::ShowTextEditor, "view.text_editor", ["V", "C"]),
        (CoreCommand::ShowViewer, "view.viewer", ["V", "V"]),
    ] {
        assert_eq!(command.id(), id);
        let definition = registry
            .commands()
            .find(|entry| entry.id == id)
            .ok_or("画面切替の固定IDがRegistryに登録されていません")?;
        assert_eq!(definition.keytips_path, path);
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
                        has_unapplied_csv_text: true,
                        ..editable_context()
                    };
                    assert_eq!(
                        definition.is_enabled(&context),
                        focus != CommandFocus::Modal
                    );
                }
            }
        }
    }
    Ok(())
}

// {
//   責務: [disabled_contexts_never_execute_the_handler: 無効なfocus・mode・document状態で実行しないことを検証する。],
//   処理: [CSVなし、履歴なし、未適用text、dialog、text focusの各拒否条件でhandlerの実行回数を調べる。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、または登録エラー。]
// }
#[test]
fn disabled_contexts_never_execute_the_handler() -> Result<(), Box<dyn Error>> {
    let registry = CommandRegistry::new(core_command_definitions())?;
    let context = editable_context();
    let cases = [
        (
            CoreCommand::Save,
            CommandContext {
                has_document: false,
                ..context
            },
        ),
        (
            CoreCommand::Save,
            CommandContext {
                has_unapplied_csv_text: true,
                ..context
            },
        ),
        (
            CoreCommand::Undo,
            CommandContext {
                has_document: false,
                ..context
            },
        ),
        (
            CoreCommand::Undo,
            CommandContext {
                can_undo: false,
                ..context
            },
        ),
        (
            CoreCommand::Redo,
            CommandContext {
                can_redo: false,
                ..context
            },
        ),
        (
            CoreCommand::Undo,
            CommandContext {
                focus: CommandFocus::TextInput,
                ..context
            },
        ),
        (
            CoreCommand::Undo,
            CommandContext {
                mode: CommandMode::TextEditor,
                ..context
            },
        ),
        (
            CoreCommand::Redo,
            CommandContext {
                mode: CommandMode::Help,
                ..context
            },
        ),
        (
            CoreCommand::Open,
            CommandContext {
                focus: CommandFocus::Modal,
                ..context
            },
        ),
    ];
    for (command, disabled_context) in cases {
        let mut calls = 0;
        assert_eq!(
            registry.dispatch(command.id(), &disabled_context, |_| {
                calls += 1;
            }),
            Err(CommandDispatchError::DisabledCommand(command.id()))
        );
        assert_eq!(calls, 0);
    }
    let no_document = CommandContext {
        has_document: false,
        ..context
    };
    let save_shortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
    assert_eq!(
        registry.dispatch_shortcut(save_shortcut, &no_document, |_| ()),
        Some(Err(CommandDispatchError::DisabledCommand("file.save")))
    );
    Ok(())
}

// {
//   責務: [unknown_ids_and_unmatched_shortcuts_do_not_execute: 未登録の入口を安全に扱うことを検証する。],
//   処理: [未知IDと一致しないkeyを渡し、handler未実行と拒否結果を確認する。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、または登録エラー。]
// }
#[test]
fn unknown_ids_and_unmatched_shortcuts_do_not_execute() -> Result<(), Box<dyn Error>> {
    let registry = CommandRegistry::new(core_command_definitions())?;
    let context = editable_context();
    let mut calls = 0;
    assert_eq!(
        registry.dispatch("file.missing", &context, |_| {
            calls += 1;
        }),
        Err(CommandDispatchError::UnknownCommand(
            "file.missing".to_owned()
        ))
    );
    assert!(
        registry
            .dispatch_shortcut(
                KeyboardShortcut::new(Modifiers::COMMAND, Key::X),
                &context,
                |_| {
                    calls += 1;
                }
            )
            .is_none()
    );
    let text_focus = CommandContext {
        focus: CommandFocus::TextInput,
        ..context
    };
    assert!(
        registry
            .dispatch_shortcut(
                KeyboardShortcut::new(Modifiers::COMMAND, Key::Z),
                &text_focus,
                |_| {
                    calls += 1;
                }
            )
            .is_none()
    );
    assert_eq!(calls, 0);
    assert_eq!(
        registry.dispatch("file.save", &text_focus, |action| *action)?,
        CoreCommand::Save
    );
    Ok(())
}

// {
//   責務: [registration_rejects_missing_metadata_and_duplicate_ids: 不完全な定義を実行前に拒否することを検証する。],
//   処理: [各必須項目を欠落させ、ID重複とともに登録失敗を確認する。],
//   引数: [],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
#[test]
fn registration_rejects_missing_metadata_and_duplicate_ids() {
    for field in [
        "id",
        "name",
        "description",
        "category",
        "scope",
        "keytips_path",
        "shortcuts",
    ] {
        let mut definition = core_command_definitions().remove(0);
        match field {
            "id" => definition.id = "invalid id",
            "name" => definition.name = " ",
            "description" => definition.description = "",
            "category" => definition.category = "",
            "scope" => definition.scope.modes = &[],
            "keytips_path" => definition.keytips_path = &[],
            "shortcuts" => definition.shortcuts.push(definition.shortcuts[0]),
            _ => unreachable!("test cases contain only known metadata fields"),
        }
        assert!(matches!(CommandRegistry::new(vec![definition]),
            Err(CommandRegistryError::InvalidMetadata { field: actual, .. }) if actual == field));
    }
    let mut definitions = core_command_definitions();
    definitions.push(definitions[0].clone());
    assert!(matches!(
        CommandRegistry::new(definitions),
        Err(CommandRegistryError::DuplicateId("file.open"))
    ));
}

// {
//   責務: [registration_rejects_shortcut_conflicts_even_when_disabled: 動的な無効化を衝突回避と誤認しないことを検証する。],
//   処理: [同じscopeで別名bindingが重なる定義を無効化しても、Registryが登録を拒否することを確認する。],
//   引数: [],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
#[test]
fn registration_rejects_shortcut_conflicts_even_when_disabled() {
    let mut definitions = core_command_definitions();
    definitions[0]
        .shortcuts
        .push(KeyboardShortcut::new(Modifiers::COMMAND, Key::Y));
    definitions[0].enabled = |_| false;
    assert!(matches!(
        CommandRegistry::new(definitions),
        Err(CommandRegistryError::ShortcutConflict { .. })
    ));
}

// {
//   責務: [alt_paths_reject_empty_invalid_and_ambiguous_sequences: Alt経路の欠落と打鍵列の曖昧さを検証する。],
//   処理: [空label、不正文字、大小文字違い、leaf-prefix、複数文字labelを含む衝突を拒否する。],
//   引数: [],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
#[test]
fn alt_paths_reject_empty_invalid_and_ambiguous_sequences() {
    let invalid_paths: [&'static [&'static str]; 3] = [&[""], &["F", " "], &["F", "O!"]];
    for path in invalid_paths {
        let mut definition = core_command_definitions().remove(0);
        definition.keytips_path = path;
        assert!(matches!(
            CommandRegistry::new(vec![definition]),
            Err(CommandRegistryError::InvalidMetadata {
                field: "keytips_path",
                ..
            })
        ));
    }
    let conflicting_paths: [&'static [&'static str]; 5] =
        [&["f", "o"], &["F"], &["F", "O", "X"], &["FO"], &["FO", "X"]];
    for path in conflicting_paths {
        let mut definitions = core_command_definitions();
        definitions[1].keytips_path = path;
        assert!(matches!(
            CommandRegistry::new(definitions),
            Err(CommandRegistryError::KeyTipsConflict { .. })
        ));
    }
}

// {
//   責務: [disjoint_modes_or_focuses_can_reuse_bindings: contextで区別できるcommandの再利用を検証する。],
//   処理: [同一bindingとAlt経路をmodeまたはfocusで分離し、それぞれ一意にdispatchできることを確認する。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、または登録・dispatchエラー。]
// }
#[test]
fn disjoint_modes_or_focuses_can_reuse_bindings() -> Result<(), Box<dyn Error>> {
    for different_focus in [false, true] {
        let mut definitions = core_command_definitions();
        definitions.truncate(2);
        definitions[0].scope = CommandScope {
            modes: &[CommandMode::TableEditor],
            focuses: &[CommandFocus::Workspace],
        };
        definitions[1].scope = if different_focus {
            CommandScope {
                modes: &[CommandMode::TableEditor],
                focuses: &[CommandFocus::TextInput],
            }
        } else {
            CommandScope {
                modes: &[CommandMode::Viewer],
                focuses: &[CommandFocus::Workspace],
            }
        };
        definitions[1].shortcuts = definitions[0].shortcuts.clone();
        definitions[1].keytips_path = definitions[0].keytips_path;
        let registry = CommandRegistry::new(definitions)?;
        let first_context = editable_context();
        let second_context = if different_focus {
            CommandContext {
                focus: CommandFocus::TextInput,
                ..first_context
            }
        } else {
            CommandContext {
                mode: CommandMode::Viewer,
                ..first_context
            }
        };
        let binding = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
        assert_eq!(
            registry.dispatch_shortcut(binding, &first_context, |action| *action),
            Some(Ok(CoreCommand::Open))
        );
        assert_eq!(
            registry.dispatch_shortcut(binding, &second_context, |action| *action),
            Some(Ok(CoreCommand::Save))
        );
    }
    Ok(())
}

// {
//   責務: [handler_failures_are_returned_without_becoming_success: adapter側の操作失敗を保持する契約を検証する。],
//   処理: [許可された保存handlerからエラーを返し、Registryが変換せず返すことを確認する。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、または登録・dispatchエラー。]
// }
#[test]
fn handler_failures_are_returned_without_becoming_success() -> Result<(), Box<dyn Error>> {
    let registry = CommandRegistry::new(core_command_definitions())?;
    let handler_result = registry.dispatch("file.save", &editable_context(), |_| {
        Err::<(), _>("save failed")
    })?;
    assert_eq!(handler_result, Err("save failed"));
    Ok(())
}

// {
//   責務: [platform_modifier_states_preserve_primary_and_redo_aliases: Ctrl/Cmdのplatform差とShift付きRedoを検証する。],
//   処理: [Windows/LinuxとmacOSの押下状態を渡し、Save・Undo・Redoを一意にdispatchする。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、または登録エラー。]
// }
#[test]
fn platform_modifier_states_preserve_primary_and_redo_aliases() -> Result<(), Box<dyn Error>> {
    let registry = CommandRegistry::new(core_command_definitions())?;
    let context = editable_context();
    for modifiers in [
        Modifiers::CTRL | Modifiers::COMMAND,
        Modifiers::MAC_CMD | Modifiers::COMMAND,
    ] {
        for (key, shift, expected) in [
            (Key::S, false, CoreCommand::Save),
            (Key::Z, false, CoreCommand::Undo),
            (Key::Z, true, CoreCommand::Redo),
        ] {
            let pressed = KeyboardShortcut::new(Modifiers { shift, ..modifiers }, key);
            assert_eq!(
                registry.dispatch_shortcut(pressed, &context, |action| *action),
                Some(Ok(expected))
            );
        }
    }
    Ok(())
}

// {
//   責務: [logical_and_physical_modifier_collisions_are_rejected: logical Commandと物理Ctrl/Cmdの衝突を検証する。],
//   処理: [同時に一致するbindingを別commandまたは同一commandの別名へ追加し、登録拒否を確認する。],
//   引数: [],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
#[test]
fn logical_and_physical_modifier_collisions_are_rejected() {
    for modifiers in [
        Modifiers::CTRL,
        Modifiers::MAC_CMD,
        Modifiers::CTRL | Modifiers::COMMAND,
    ] {
        let mut definitions = core_command_definitions();
        definitions[1].shortcuts = vec![KeyboardShortcut::new(modifiers, Key::O)];
        assert!(matches!(
            CommandRegistry::new(definitions),
            Err(CommandRegistryError::ShortcutConflict { .. })
        ));
    }
    let mut definition = core_command_definitions().remove(0);
    definition
        .shortcuts
        .push(KeyboardShortcut::new(Modifiers::CTRL, Key::O));
    assert!(matches!(
        CommandRegistry::new(vec![definition]),
        Err(CommandRegistryError::InvalidMetadata {
            field: "shortcuts",
            ..
        })
    ));
}
