use eframe::egui::{Key, KeyboardShortcut, Modifiers};

use super::{CommandDefinition, CommandFocus, CommandMode, CommandScope};

// {
//   責務: [CoreCommand: Registryの最初のadapter接続対象となる既存GUI操作を識別する。],
//   フィールド: [Open: path欄のCSVを開く, Save: CSVを保存, Undo/Redo: CSV編集履歴の移動。]
// }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreCommand {
    Open,
    Save,
    Undo,
    Redo,
}

impl CoreCommand {
    // {
    //   責務: [id: 基本GUI操作のstable IDを返す。],
    //   処理: [操作variantを固定文字列へ対応付ける。],
    //   引数: [self: 対象操作。],
    //   戻り値: [&'static str: 外部入口が共通に利用するID。]
    // }
    pub const fn id(self) -> &'static str {
        match self {
            Self::Open => "file.open",
            Self::Save => "file.save",
            Self::Undo => "edit.undo",
            Self::Redo => "edit.redo",
        }
    }
}

// {
//   責務: [core_command_definitions: Open/Save/Undo/Redoの共通metadataを作る。],
//   処理: [scope・動的enabled条件・shortcut・Alt階層を各操作へ設定する。],
//   引数: [],
//   戻り値: [Vec<CommandDefinition<CoreCommand>>: Registryへ登録する4定義。],
//   補足: [実GUIへ接続する前の契約。text widgetのUndo/RedoはCSV履歴へ渡さない。]
// }
pub fn core_command_definitions() -> Vec<CommandDefinition<CoreCommand>> {
    let file_scope = CommandScope {
        modes: CommandScope::ALL.modes,
        focuses: &[CommandFocus::Workspace, CommandFocus::TextInput],
    };
    let history_scope = CommandScope {
        modes: &[CommandMode::TableEditor, CommandMode::Viewer],
        focuses: &[CommandFocus::Workspace],
    };
    vec![
        CommandDefinition {
            id: CoreCommand::Open.id(),
            name: "CSVを開く",
            description: "path欄に指定したCSVを開きます。",
            category: "ファイル",
            scope: file_scope,
            enabled: |_| true,
            shortcuts: vec![KeyboardShortcut::new(Modifiers::COMMAND, Key::O)],
            keytips_path: &["F", "O"],
            action: CoreCommand::Open,
        },
        CommandDefinition {
            id: CoreCommand::Save.id(),
            name: "CSVを保存",
            description: "開いているCSVを保存します。未適用のCSV text変更がある場合は無効です。",
            category: "ファイル",
            scope: file_scope,
            enabled: |context| context.has_document && !context.has_unapplied_csv_text,
            shortcuts: vec![KeyboardShortcut::new(Modifiers::COMMAND, Key::S)],
            keytips_path: &["F", "S"],
            action: CoreCommand::Save,
        },
        CommandDefinition {
            id: CoreCommand::Undo.id(),
            name: "CSV編集を元に戻す",
            description: "CSVに対する直前の編集を取り消します。",
            category: "編集",
            scope: history_scope,
            enabled: |context| context.has_document && context.can_undo,
            shortcuts: vec![KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)],
            keytips_path: &["E", "U"],
            action: CoreCommand::Undo,
        },
        CommandDefinition {
            id: CoreCommand::Redo.id(),
            name: "CSV編集をやり直す",
            description: "取り消したCSV編集を再適用します。",
            category: "編集",
            scope: history_scope,
            enabled: |context| context.has_document && context.can_redo,
            shortcuts: vec![
                KeyboardShortcut::new(Modifiers::COMMAND, Key::Y),
                KeyboardShortcut::new(
                    Modifiers {
                        shift: true,
                        ..Modifiers::COMMAND
                    },
                    Key::Z,
                ),
            ],
            keytips_path: &["E", "R"],
            action: CoreCommand::Redo,
        },
    ]
}
