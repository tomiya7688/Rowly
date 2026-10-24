use std::collections::BTreeMap;

use eframe::egui;

use crate::gui_commands::{
    CommandContext, CommandFocus, CommandMode, CommandRegistry, CoreCommand,
};

use super::RowlyApp;

// {
//   責務: [KeyTipsState: 基本操作のAlt階層選択を保持し、Registryから次の候補を解決する。],
//   フィールド: [active: 階層操作中か。 path: 選択済みlabel。 prefix: 複数文字labelの入力途中。 alt_down: 前frameのAlt状態。 context: 操作開始時のmode・focus種別とwidget ID。]
// }
#[derive(Default)]
pub(super) struct KeyTipsState {
    active: bool,
    path: Vec<String>,
    prefix: String,
    alt_down: bool,
    context: Option<(CommandMode, CommandFocus, Option<egui::Id>)>,
}

impl KeyTipsState {
    // {
    //   責務: [cancel: KeyTipsの選択途中状態を破棄する。],
    //   処理: [Alt押下追跡を維持し、選択経路と操作contextを初期化する。],
    //   引数: [self: 操作中の階層状態。],
    //   戻り値: [(): 通常入力へ戻る。]
    // }
    fn cancel(&mut self) {
        self.active = false;
        self.path.clear();
        self.prefix.clear();
        self.context = None;
    }

    // {
    //   責務: [hints: 現在階層のlabel・説明・有効状態をRegistryから列挙する。],
    //   処理: [scope内の操作を経路prefixで絞り、共通の親labelをまとめる。],
    //   引数: [self: 選択階層。 registry: 基本操作定義。 context: 現在のmode・focus・CSV状態。],
    //   戻り値: [Vec: 次のlabel、説明、配下に有効操作があるかの組。]
    // }
    fn hints(
        &self,
        registry: &CommandRegistry<CoreCommand>,
        context: &CommandContext,
    ) -> Vec<(&'static str, &'static str, bool)> {
        let mut hints = BTreeMap::new();
        for command in registry
            .commands()
            .filter(|entry| entry.scope.allows(context))
        {
            if !command
                .keytips_path
                .iter()
                .zip(&self.path)
                .all(|(left, right)| left.eq_ignore_ascii_case(right))
                || command.keytips_path.len() <= self.path.len()
            {
                continue;
            }
            let label = command.keytips_path[self.path.len()];
            let name = if self.path.len() + 1 == command.keytips_path.len() {
                command.name
            } else {
                command.category
            };
            let enabled = command.is_enabled(context);
            let entry = hints.entry(label).or_insert((name, false));
            entry.1 |= enabled;
        }
        hints
            .into_iter()
            .map(|(label, (name, enabled))| (label, name, enabled))
            .collect()
    }

    // {
    //   責務: [advance: 入力文字から階層を進め、leafの操作を解決する。],
    //   処理: [labelを大文字小文字を区別せず照合し、実行leafでは選択状態を終了する。],
    //   引数: [self: 選択途中状態。 registry: 経路の正本。 context: 現在の有効条件。 key: 英数字の一文字。],
    //   戻り値: [Option<CoreCommand>: leaf一致時の操作。実行可否は共通dispatchで再検査する。]
    // }
    fn advance(
        &mut self,
        registry: &CommandRegistry<CoreCommand>,
        context: &CommandContext,
        key: &str,
    ) -> Option<CoreCommand> {
        self.prefix.push_str(key);
        let hints = self.hints(registry, context);
        let Some((label, _, _)) = hints
            .iter()
            .find(|(label, _, _)| label.eq_ignore_ascii_case(&self.prefix))
        else {
            if !hints.iter().any(|(label, _, _)| {
                label
                    .to_ascii_uppercase()
                    .starts_with(&self.prefix.to_ascii_uppercase())
            }) {
                self.prefix.clear();
            }
            return None;
        };
        self.path.push((*label).to_owned());
        self.prefix.clear();
        let command = registry
            .commands()
            .find(|entry| {
                entry.scope.allows(context)
                    && entry.keytips_path.len() == self.path.len()
                    && entry
                        .keytips_path
                        .iter()
                        .zip(&self.path)
                        .all(|(left, right)| left.eq_ignore_ascii_case(right))
            })
            .map(|entry| entry.action);
        if command.is_some() {
            self.cancel();
        }
        command
    }
}

impl RowlyApp {
    // {
    //   責務: [dispatch_keytips: Alt開始・階層入力・取消を処理し、leafを共通dispatchへ渡す。],
    //   処理: [focus・modal・pointer変更で終了し、選択文字をwidgetへ配送せず操作をlock外で実行する。],
    //   引数: [self: GUIと階層状態。 ctx: 現在frameの入力。],
    //   戻り値: [(): 登録済みの基本操作を実行するか、Alt階層の状態を更新する。]
    // }
    pub(super) fn dispatch_keytips(&mut self, ctx: &egui::Context) {
        let context = self.command_context(ctx);
        let focused_widget = ctx.memory(|memory| memory.focused());
        let Ok(registry) = &self.command_registry else {
            return;
        };
        let mut commands = Vec::new();
        ctx.input_mut(|input| {
            let state = &mut self.keytips;
            let alt_pressed =
                input.modifiers.matches_exact(egui::Modifiers::ALT) && !state.alt_down;
            state.alt_down = input.modifiers.alt;
            if !input.focused
                || context.focus == CommandFocus::Modal
                || input.modifiers.ctrl
                || input.modifiers.command
                || input.modifiers.shift
                || input.pointer.any_pressed()
                || state.context.is_some_and(|previous| {
                    previous != (context.mode, context.focus, focused_widget)
                })
            {
                state.cancel();
                return;
            }
            if alt_pressed {
                if state.active {
                    state.cancel();
                } else {
                    state.active = true;
                    state.context = Some((context.mode, context.focus, focused_widget));
                }
            }
            let mut selected_text = None;
            input.events.retain(|event| {
                if matches!(event, egui::Event::Ime(_)) && state.active {
                    // 日本語入力へ移った場合は階層操作を終了し、IMEへ入力を返す。
                    state.cancel();
                    return true;
                }
                if let egui::Event::Text(text) = event {
                    // leaf選択に伴う文字だけを除去し、終了後に入力された別の文字は残す。
                    let captured = state.active
                        || selected_text
                            .as_deref()
                            .is_some_and(|selected: &str| selected.eq_ignore_ascii_case(text));
                    selected_text = None;
                    return !captured;
                }
                let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat,
                    modifiers,
                    ..
                } = event
                else {
                    return true;
                };
                if !state.active {
                    return true;
                }
                if modifiers.ctrl || modifiers.command {
                    state.cancel();
                    return true;
                }
                if *repeat {
                    return false;
                }
                if *key == egui::Key::Escape {
                    if !state.prefix.is_empty() {
                        state.prefix.clear();
                    } else if state.path.pop().is_none() {
                        state.cancel();
                    }
                } else if key.name().len() == 1
                    && key.name().bytes().all(|byte| byte.is_ascii_alphanumeric())
                {
                    selected_text = Some(key.name().to_owned());
                    if let Some(command) = state.advance(registry, &context, key.name()) {
                        commands.push(command);
                    }
                } else {
                    state.cancel();
                    return true;
                }
                false
            });
        });
        for command in commands {
            self.dispatch_core_command(ctx, command);
        }
    }

    // {
    //   責務: [show_keytips: メニュー直下に現在階層のキーと操作候補を重ねて表示する。],
    //   処理: [Registryの候補を使い、実行不能な操作をdisabled表示する。],
    //   引数: [self: 階層状態とRegistry。 ctx: overlay描画先。],
    //   戻り値: [(): 操作中だけキーの案内を描画する。]
    // }
    pub(super) fn show_keytips(&self, ctx: &egui::Context) {
        if !self.keytips.active {
            return;
        }
        let Ok(registry) = &self.command_registry else {
            return;
        };
        let hints = self.keytips.hints(registry, &self.command_context(ctx));
        egui::Area::new(egui::Id::new("core_keytips"))
            .fixed_pos(egui::pos2(8.0, 28.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label("Alt キー操作（登録済み操作） / Escで戻る");
                    for (key, name, enabled) in hints {
                        ui.add_enabled(enabled, egui::Label::new(format!("[{key}] {name}")));
                    }
                });
            });
    }
}
