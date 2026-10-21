use std::collections::BTreeMap;

use eframe::egui::KeyboardShortcut;
use thiserror::Error;

use super::{CommandContext, CommandScope};

// {
//   責務: [CommandDefinition: 複数のGUI入口が参照するcommand定義を一箇所に保持する。],
//   フィールド: [id: stable識別子, name/description/category: 表示と検索用text, scope: 許可context, enabled: 動的な状態検査, shortcuts: 専用bindingと別名binding, keytips_path: 必須のAlt階層metadata, action: adapterへ渡す実行操作。]
// }
#[derive(Clone)]
pub struct CommandDefinition<Action> {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub scope: CommandScope,
    pub enabled: fn(&CommandContext) -> bool,
    pub shortcuts: Vec<KeyboardShortcut>,
    pub keytips_path: &'static [&'static str],
    pub action: Action,
}

impl<Action> CommandDefinition<Action> {
    // {
    //   責務: [is_enabled: scopeと実行直前の状態からcommandの有効性を返す。],
    //   処理: [scopeを満たす場合だけenabled predicateを呼ぶ。],
    //   引数: [self: 対象定義, context: 実行直前のGUI状態。],
    //   戻り値: [bool: contextと状態の両方を満たす場合にtrue。]
    // }
    pub fn is_enabled(&self, context: &CommandContext) -> bool {
        self.scope.allows(context) && (self.enabled)(context)
    }
}

// {
//   責務: [CommandRegistryError: 実行前に拒否する登録情報の不整合を識別する。],
//   フィールド: [InvalidMetadata: 必須項目の欠落または不正値, DuplicateId: ID重複, ShortcutConflict/KeyTipsConflict: 共通contextでの入口衝突。]
// }
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandRegistryError {
    #[error("command `{id}` のmetadata `{field}` が不正です")]
    InvalidMetadata {
        id: &'static str,
        field: &'static str,
    },
    #[error("command ID `{0}` が重複しています")]
    DuplicateId(&'static str),
    #[error("command `{first}` と `{second}` のshortcutが衝突しています")]
    ShortcutConflict {
        first: &'static str,
        second: &'static str,
    },
    #[error("command `{first}` と `{second}` のAlt経路が衝突しています")]
    KeyTipsConflict {
        first: &'static str,
        second: &'static str,
    },
}

// {
//   責務: [CommandDispatchError: command実行前のID・context検査失敗を返す。],
//   フィールド: [UnknownCommand: 未登録ID, DisabledCommand: 現在のcontextでは実行できないID。]
// }
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandDispatchError {
    #[error("command `{0}` は登録されていません")]
    UnknownCommand(String),
    #[error("command `{0}` は現在の操作状態では実行できません")]
    DisabledCommand(&'static str),
}

// {
//   責務: [CommandRegistry: 検証済み定義と複数入口からの共通dispatchを所有する。],
//   フィールド: [commands: 表示順の定義, by_id: stable IDから定義位置への索引。]
// }
pub struct CommandRegistry<Action> {
    commands: Vec<CommandDefinition<Action>>,
    by_id: BTreeMap<&'static str, usize>,
}

impl<Action> CommandRegistry<Action> {
    // {
    //   責務: [new: 必須metadataと入口の衝突を検証してRegistryを作る。],
    //   処理: [ID索引を作り、scopeが重なる定義のshortcutとAlt経路を照合する。],
    //   引数: [commands: 登録する全定義。],
    //   戻り値: [Result<Self, CommandRegistryError>: Registry、または登録エラー。],
    //   補足: [動的enabled predicateの相互排他は衝突回避の根拠にしない。]
    // }
    pub fn new(commands: Vec<CommandDefinition<Action>>) -> Result<Self, CommandRegistryError> {
        let mut by_id = BTreeMap::new();
        for (index, command) in commands.iter().enumerate() {
            validate_metadata(command)?;
            if by_id.insert(command.id, index).is_some() {
                return Err(CommandRegistryError::DuplicateId(command.id));
            }
            // 登録時だけ全組を照合する。実行時のID検索は索引から行う。
            for previous in &commands[..index] {
                if !command.scope.overlaps(previous.scope) {
                    continue;
                }
                if command.shortcuts.iter().any(|shortcut| {
                    previous
                        .shortcuts
                        .iter()
                        .any(|previous_shortcut| shortcuts_conflict(*shortcut, *previous_shortcut))
                }) {
                    return Err(CommandRegistryError::ShortcutConflict {
                        first: previous.id,
                        second: command.id,
                    });
                }
                if keytips_paths_conflict(command.keytips_path, previous.keytips_path) {
                    return Err(CommandRegistryError::KeyTipsConflict {
                        first: previous.id,
                        second: command.id,
                    });
                }
            }
        }
        Ok(Self { commands, by_id })
    }

    // {
    //   責務: [commands: menu・palette・KeyTipsなどの生成元となる定義を列挙する。],
    //   処理: [登録順の読み取り専用iteratorを返す。],
    //   引数: [self: 対象Registry。],
    //   戻り値: [impl ExactSizeIterator: 登録済み定義への参照。]
    // }
    pub fn commands(&self) -> impl ExactSizeIterator<Item = &CommandDefinition<Action>> {
        self.commands.iter()
    }

    // {
    //   責務: [dispatch: stable IDを解決し、実行直前の有効性を確認してhandlerを呼ぶ。],
    //   処理: [未知IDと無効contextを拒否し、許可されたactionだけをadapterへ渡す。],
    //   引数: [self: Registry, id: 実行ID, context: 現在のGUI状態, execute: adapterの実行handler。],
    //   戻り値: [Result<Output, CommandDispatchError>: handlerの戻り値、または実行前の拒否理由。],
    //   補足: [handlerがResultを返す場合も、その値を変更せず呼び出し元へ返す。]
    // }
    pub fn dispatch<Output>(
        &self,
        id: &str,
        context: &CommandContext,
        execute: impl FnOnce(&Action) -> Output,
    ) -> Result<Output, CommandDispatchError> {
        let index = self
            .by_id
            .get(id)
            .ok_or_else(|| CommandDispatchError::UnknownCommand(id.to_owned()))?;
        let command = &self.commands[*index];
        if !command.is_enabled(context) {
            return Err(CommandDispatchError::DisabledCommand(command.id));
        }
        Ok(execute(&command.action))
    }

    // {
    //   責務: [dispatch_shortcut: 登録shortcutとscopeから対象を選び、IDと同じdispatchへ渡す。],
    //   処理: [shortcutとscopeが一致する定義を選び、動的enabled検査を共通経路で行う。],
    //   引数: [self: Registry, shortcut: eguiが正規化した修飾keyと押下key, context: 現在状態, execute: adapterのhandler。],
    //   戻り値: [Option<Result<Output, CommandDispatchError>>: 一致なしはNone、一致時はdispatch結果。],
    //   補足: [Shift/Altは完全一致を要求する。Ctrl+ZでCtrl+Shift+Zまで消費しない。]
    // }
    pub fn dispatch_shortcut<Output>(
        &self,
        shortcut: KeyboardShortcut,
        context: &CommandContext,
        execute: impl FnOnce(&Action) -> Output,
    ) -> Option<Result<Output, CommandDispatchError>> {
        self.commands
            .iter()
            .find(|command| {
                command
                    .shortcuts
                    .iter()
                    .any(|binding| shortcut_matches(shortcut, *binding))
                    && command.scope.allows(context)
            })
            .map(|command| self.dispatch(command.id, context, execute))
    }
}

// {
//   責務: [validate_metadata: 表示とkeyboard経路に必要な項目の欠落を拒否する。],
//   処理: [文字列・scope・Alt階層の各値を検証する。],
//   引数: [command: 検証対象の定義。],
//   戻り値: [Result<(), CommandRegistryError>: 成功、または不正項目名。]
// }
fn validate_metadata<Action>(
    command: &CommandDefinition<Action>,
) -> Result<(), CommandRegistryError> {
    let invalid_field = if command.id.is_empty() || command.id.chars().any(char::is_whitespace) {
        Some("id")
    } else if command.name.trim().is_empty() {
        Some("name")
    } else if command.description.trim().is_empty() {
        Some("description")
    } else if command.category.trim().is_empty() {
        Some("category")
    } else if command.scope.modes.is_empty() || command.scope.focuses.is_empty() {
        Some("scope")
    } else if command
        .shortcuts
        .iter()
        .enumerate()
        .any(|(index, shortcut)| {
            command.shortcuts[..index]
                .iter()
                .any(|previous| shortcuts_conflict(*shortcut, *previous))
        })
    {
        Some("shortcuts")
    } else if command.keytips_path.is_empty()
        || command.keytips_path.iter().any(|segment| {
            segment.is_empty() || !segment.bytes().all(|key| key.is_ascii_alphanumeric())
        })
    {
        Some("keytips_path")
    } else {
        None
    };
    match invalid_field {
        Some(field) => Err(CommandRegistryError::InvalidMetadata {
            id: command.id,
            field,
        }),
        None => Ok(()),
    }
}

// {
//   責務: [keytips_paths_conflict: 同じAlt経路、同階層のprefix label、leafと子階層の衝突を検出する。],
//   処理: [共通階層を辿り、同階層で短いlabelが先に確定してしまう曖昧な経路を拒否する。],
//   引数: [left: 一方の経路, right: 他方の経路。],
//   戻り値: [bool: キー列の選択を一意に決められない場合にtrue。]
// }
fn keytips_paths_conflict(left: &[&str], right: &[&str]) -> bool {
    for (left_label, right_label) in left.iter().zip(right) {
        if left_label.eq_ignore_ascii_case(right_label) {
            continue;
        }
        // FとFOのようなlabelは、Fの時点で階層移動するか追加入力を待つか決められない。
        return left_label
            .bytes()
            .zip(right_label.bytes())
            .all(|(left_key, right_key)| left_key.eq_ignore_ascii_case(&right_key));
    }
    true
}

// {
//   責務: [shortcut_matches: platform依存のCommand修飾keyを解決し、押下keyとbindingを照合する。],
//   処理: [eguiの完全一致判定でCtrl/Cmdを扱い、Shift/Altの過不足を拒否する。],
//   引数: [pressed: eguiの押下状態, binding: 登録済みshortcut。],
//   戻り値: [bool: keyと修飾keyが一致する場合にtrue。]
// }
fn shortcut_matches(pressed: KeyboardShortcut, binding: KeyboardShortcut) -> bool {
    pressed.logical_key == binding.logical_key && pressed.modifiers.matches_exact(binding.modifiers)
}

// {
//   責務: [shortcuts_conflict: Windows/LinuxまたはmacOSで同じ押下に一致するbindingを検出する。],
//   処理: [keyとShift/Altを比較し、Ctrl・Cmdの実際の状態を両方のbindingへ照合する。],
//   引数: [left: 一方のbinding, right: 他方のbinding。],
//   戻り値: [bool: どちらかのplatformで同時に一致する押下がある場合にtrue。]
// }
fn shortcuts_conflict(left: KeyboardShortcut, right: KeyboardShortcut) -> bool {
    use eframe::egui::Modifiers;
    if left.logical_key != right.logical_key
        || left.modifiers.shift != right.modifiers.shift
        || left.modifiers.alt != right.modifiers.alt
    {
        return false;
    }
    // Win/LinuxではCommand=Ctrl、macOSではCommand=Cmd。macOSのCtrl単独とCtrl+Cmdも含む。
    let platform_states = [
        Modifiers::NONE,
        Modifiers::CTRL | Modifiers::COMMAND,
        Modifiers::CTRL,
        Modifiers::MAC_CMD | Modifiers::COMMAND,
        Modifiers::CTRL | Modifiers::MAC_CMD | Modifiers::COMMAND,
    ];
    platform_states.into_iter().any(|modifiers| {
        let pressed = KeyboardShortcut::new(
            Modifiers {
                shift: left.modifiers.shift,
                alt: left.modifiers.alt,
                ..modifiers
            },
            left.logical_key,
        );
        shortcut_matches(pressed, left) && shortcut_matches(pressed, right)
    })
}
