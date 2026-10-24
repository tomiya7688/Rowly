use eframe::egui::{Key, KeyboardShortcut, Modifiers};

use super::{CommandDefinition, CommandFocus, CommandMode, CommandScope};

// {
//   責務: [CoreCommand: 共通Registryを通して実行するGUIの基本操作を識別する。],
//   フィールド: [Open: path欄のCSVまたはprojectを開く。 Save: CSVとprojectの生成設定を保存する。 Undo/Redo: CSV編集履歴を移動する。 ShowTableEditor/ShowTextEditor/ShowViewer: 指定した作業画面を表示する。]
// }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreCommand {
    Open,
    Save,
    Undo,
    Redo,
    ShowTableEditor,
    ShowTextEditor,
    ShowViewer,
}

impl CoreCommand {
    // {
    //   責務: [WORKSPACE_SWITCHES: menuとtoolbarで提供する画面切替操作を表示順に定義する。]
    // }
    pub const WORKSPACE_SWITCHES: [Self; 3] = [
        Self::ShowTableEditor,
        Self::ShowTextEditor,
        Self::ShowViewer,
    ];

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
            Self::ShowTableEditor => "view.table_editor",
            Self::ShowTextEditor => "view.text_editor",
            Self::ShowViewer => "view.viewer",
        }
    }
}

// {
//   責務: [core_command_definitions: ファイル・履歴・画面切替の共通metadataを作る。],
//   処理: [scope・動的enabled条件・shortcut・Alt階層を各操作へ設定する。],
//   引数: [],
//   戻り値: [Vec<CommandDefinition<CoreCommand>>: Registryへ登録する基本操作の定義。],
//   補足: [GUI adapterは同じ定義を表示と実行に使う。text widgetのUndo/RedoはCSV履歴へ渡さない。]
// }
pub fn core_command_definitions() -> Vec<CommandDefinition<CoreCommand>> {
    let non_modal_scope = CommandScope {
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
            name: "ファイルを開く",
            description: "path欄に指定したCSVまたはprojectを開きます。",
            category: "ファイル",
            scope: non_modal_scope,
            enabled: |_| true,
            shortcuts: vec![KeyboardShortcut::new(Modifiers::COMMAND, Key::O)],
            keytips_path: &["F", "O"],
            action: CoreCommand::Open,
        },
        CommandDefinition {
            id: CoreCommand::Save.id(),
            name: "変更を保存",
            description: "開いているCSVを保存し、project利用時は計算規則も保存します。未適用のCSV text変更がある場合は無効です。",
            category: "ファイル",
            scope: non_modal_scope,
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
        CommandDefinition {
            id: CoreCommand::ShowTableEditor.id(),
            name: "Table Editor",
            description: "セルと行列を編集する表画面を表示します。",
            category: "表示",
            scope: non_modal_scope,
            enabled: |_| true,
            shortcuts: vec![],
            keytips_path: &["V", "E"],
            action: CoreCommand::ShowTableEditor,
        },
        CommandDefinition {
            id: CoreCommand::ShowTextEditor.id(),
            name: "Text Editor",
            description: "CSVテキストの編集画面を表示します。未適用のテキストは保持します。",
            category: "表示",
            scope: non_modal_scope,
            enabled: |_| true,
            shortcuts: vec![],
            keytips_path: &["V", "C"],
            action: CoreCommand::ShowTextEditor,
        },
        CommandDefinition {
            id: CoreCommand::ShowViewer.id(),
            name: "Viewer",
            description: "CSVの値を変更せず、表の閲覧画面を表示します。",
            category: "表示",
            scope: non_modal_scope,
            enabled: |_| true,
            shortcuts: vec![],
            keytips_path: &["V", "V"],
            action: CoreCommand::ShowViewer,
        },
    ]
}
