// {
//   責務: [CommandMode: commandが利用されるworkspaceの種類を識別する。],
//   フィールド: [TableEditor: 表編集, TextEditor: CSV text編集, Viewer: 表閲覧, Help: ヘルプ表示。]
// }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandMode {
    TableEditor,
    TextEditor,
    Viewer,
    Help,
}

// {
//   責務: [CommandFocus: workspace commandとwidget固有のkey操作を分離する。],
//   フィールド: [Workspace: 表などの通常領域, TextInput: text入力中, Modal: dialog内の操作中。]
// }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandFocus {
    Workspace,
    TextInput,
    Modal,
}

// {
//   責務: [CommandContext: command実行直前のGUI状態を読み取り専用の値として渡す。],
//   フィールド: [mode: workspace, focus: 操作領域, has_document: CSVの有無, can_undo/can_redo: CSV履歴の可否, has_unapplied_csv_text: 未適用のCSV text編集の有無。]
// }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandContext {
    pub mode: CommandMode,
    pub focus: CommandFocus,
    pub has_document: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_unapplied_csv_text: bool,
}

// {
//   責務: [CommandScope: commandを提供するmodeとfocusの組を定義する。],
//   フィールド: [modes: 許可するworkspace, focuses: 許可する操作領域。両方を満たすcontextだけを許可する。]
// }
#[derive(Clone, Copy, Debug)]
pub struct CommandScope {
    pub modes: &'static [CommandMode],
    pub focuses: &'static [CommandFocus],
}

impl CommandScope {
    // {
    //   責務: [ALL: 全workspace・全focusを対象とするscopeを提供する。]
    // }
    pub const ALL: Self = Self {
        modes: &[
            CommandMode::TableEditor,
            CommandMode::TextEditor,
            CommandMode::Viewer,
            CommandMode::Help,
        ],
        focuses: &[
            CommandFocus::Workspace,
            CommandFocus::TextInput,
            CommandFocus::Modal,
        ],
    };

    // {
    //   責務: [allows: 現在のcontextが宣言されたscopeに含まれるか判定する。],
    //   処理: [modeとfocusが両方とも許可リストに含まれることを確認する。],
    //   引数: [self: 対象scope, context: 実行直前のGUI状態。],
    //   戻り値: [bool: 両条件を満たす場合にtrue。]
    // }
    pub fn allows(self, context: &CommandContext) -> bool {
        self.modes.contains(&context.mode) && self.focuses.contains(&context.focus)
    }

    // {
    //   責務: [overlaps: 二つのcommandが同じmodeとfocusで候補になり得るか判定する。],
    //   処理: [mode集合とfocus集合の両方に共通要素があるか調べる。],
    //   引数: [self: 一方のscope, other: 他方のscope。],
    //   戻り値: [bool: 共通contextがある場合にtrue。]
    // }
    pub(super) fn overlaps(self, other: Self) -> bool {
        self.modes.iter().any(|mode| other.modes.contains(mode))
            && self
                .focuses
                .iter()
                .any(|focus| other.focuses.contains(focus))
    }
}
