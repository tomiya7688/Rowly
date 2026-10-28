use crate::gui_commands::CoreCommand;
use eframe::egui;

// {
//   責務: [CALCULATION_EXPRESSION_WIDGET_ID: 計算式入力欄のfocusをGUI操作の条件から識別する。]
// }
pub(super) const CALCULATION_EXPRESSION_WIDGET_ID: &str = "calculation-expression-input";

use crate::{
    gui::{CalculationCommand, RowlyApp, WorkspaceMode},
    process::{
        CellRange,
        calculation::{
            CalculationBindingId, CalculationStatus, CalculationTarget, CalculationTrigger,
        },
    },
    rowly_dsl::Statement,
};

impl RowlyApp {
    // {
    //   責務: [show_calculation_panel: 選択cell/rangeのcalculation式、trigger、依存、結果操作を表示する。]
    //   引数: [self: GUI・CalculationEngine状態。 ui: panel描画先。]
    //   戻り値: [(): 設定操作と状態表示を描画する。]
    // }
    pub(super) fn show_calculation_panel(&mut self, ui: &mut egui::Ui) {
        let selected_range = self.selected_calculation_range();
        let expression_target = selected_range.start();
        if self.calculation_editor_target != Some(expression_target) {
            self.calculation_editor_target = Some(expression_target);
            let binding_id = CalculationBindingId::new(format!("dsl:{expression_target}")).ok();
            if let Some(binding) = binding_id
                .as_ref()
                .and_then(|binding_id| self.calculation_engine.binding(binding_id))
            {
                self.calculation_expression = binding.expression().to_dsl_source();
                self.calculation_trigger = binding.trigger();
            } else {
                self.calculation_expression.clear();
                self.calculation_trigger = CalculationTrigger::DependencyChange;
            }
        }
        let mut apply_requested = false;
        let mut recalculate_requested = false;
        let mut remove_requested = false;
        let mut show_raw_csv_requested = false;
        // 描画中は計算式を編集するため、先にRegistryから表示情報と有効条件を取得する。
        let (recalculate_button, remove_button) = {
            let button_info = |command| {
                self.command_registry.as_ref().ok().and_then(|registry| {
                    registry
                        .commands()
                        .find(|entry| entry.action == command)
                        .map(|entry| {
                            (
                                entry.name,
                                entry.description,
                                entry.is_enabled(&self.command_context(ui.ctx())),
                                entry
                                    .shortcuts
                                    .first()
                                    .map(|shortcut| ui.ctx().format_shortcut(shortcut)),
                            )
                        })
                })
            };
            (
                button_info(CoreCommand::Recalculate),
                button_info(CoreCommand::RemoveCalculationBinding),
            )
        };
        let raw_csv_value = self
            .document
            .as_ref()
            .and_then(|document| document.cell_ref(expression_target))
            .unwrap_or("");
        ui.group(|ui| {
            ui.label(format!("Calculation for {selected_range}"));
            ui.label(format!("CSV raw value ({expression_target}): {raw_csv_value}"));
            if ui.button("Open raw CSV in Code").clicked() {
                show_raw_csv_requested = true;
            }
            ui.horizontal(|ui| {
                ui.label("Expression");
                let expression_response = ui.add(
                    egui::TextEdit::singleline(&mut self.calculation_expression)
                        .id(egui::Id::new(CALCULATION_EXPRESSION_WIDGET_ID))
                        .hint_text("CellValue(\"A1\") * Integer(2)")
                        .desired_width(300.0),
                );
                let apply_shortcut =
                    ui.input(|input| {
                        let command = input.modifiers.command;
                        command && input.key_pressed(egui::Key::Enter)
                    });
                if expression_response.has_focus() && apply_shortcut {
                    apply_requested = true;
                }
                if expression_response.lost_focus()
                    && ui.input(|input| input.key_pressed(egui::Key::Enter))
                {
                    apply_requested = true;
                }
                egui::ComboBox::from_id_salt("calculation_trigger")
                    .selected_text(match self.calculation_trigger {
                        CalculationTrigger::DependencyChange => "Automatic",
                        CalculationTrigger::Manual => "Manual",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.calculation_trigger,
                            CalculationTrigger::DependencyChange,
                            "Automatic",
                        );
                        ui.selectable_value(
                            &mut self.calculation_trigger,
                            CalculationTrigger::Manual,
                            "Manual",
                        );
                    });
                if ui.button("計算規則を登録").clicked() {
                    apply_requested = true;
                }
                if let Some((name, description, enabled, shortcut)) = &recalculate_button {
                    let mut button = egui::Button::new(*name);
                    if let Some(shortcut) = shortcut { button = button.shortcut_text(shortcut); }
                    if ui.add_enabled(*enabled, button).on_hover_text(*description).clicked() {
                        recalculate_requested = true;
                    }
                }
                if let Some((name, description, enabled, shortcut)) = &remove_button {
                    let mut button = egui::Button::new(*name);
                    if let Some(shortcut) = shortcut { button = button.shortcut_text(shortcut); }
                    if ui.add_enabled(*enabled, button).on_hover_text(*description).clicked() {
                        remove_requested = true;
                    }
                }
            });
            let binding_id = CalculationBindingId::new(format!("dsl:{expression_target}")).ok();
            if let Some(result) = binding_id
                .as_ref()
                .and_then(|binding_id| self.calculation_engine.result(binding_id))
            {
                ui.label(match &result.status {
                    CalculationStatus::Evaluated => format!("Result: {}", result.value.as_deref().unwrap_or("")),
                    CalculationStatus::Pending => "Result: pending".to_owned(),
                    CalculationStatus::Stale => format!("Result: {} (stale)", result.value.as_deref().unwrap_or("")),
                    CalculationStatus::Error(error) => format!("Calculation error: {error}"),
                });
            }
            let dependencies = binding_id
                .as_ref()
                .and_then(|id| self.calculation_engine.binding(id))
                .map(|binding| {
                    binding
                        .dependencies()
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                });
            if let Some(dependencies) = dependencies {
                ui.label(format!("Dependencies: {dependencies}"));
            }
            if self.project_session.is_none() {
                ui.weak("CSV単体では計算規則を保存できません。projectを開くとgenerated.rlyへ保存します。");
            }
            ui.weak("Shortcuts: Ctrl+Enter bind · F9 recalculate · Ctrl+Shift+K unbind · Ctrl+Shift+F show editor");
        });
        if apply_requested {
            self.dispatch_calculation_command(CalculationCommand::Bind);
        }
        if recalculate_requested {
            self.dispatch_core_command(ui.ctx(), CoreCommand::Recalculate);
        }
        if remove_requested {
            self.dispatch_core_command(ui.ctx(), CoreCommand::RemoveCalculationBinding);
        }
        if show_raw_csv_requested {
            if !self.code_buffer_dirty {
                self.refresh_code_buffer();
            }
            self.mode = WorkspaceMode::TextEditor;
            self.status = format!("Codeで {expression_target} のraw CSV値を確認できます");
        }
    }

    // {
    //   責務: [dispatch_calculation_command: 登録済みCalculation commandをGUI操作へ対応付ける。]
    //   引数: [command: 登録一覧から選択したCalculation command。]
    //   戻り値: [(): 指定commandを実行する。]
    // }
    pub(super) fn dispatch_calculation_command(&mut self, command: CalculationCommand) {
        match command {
            CalculationCommand::Bind => {
                if self.apply_calculation_expression() {
                    self.calculation_bindings_dirty = true;
                }
            }
            CalculationCommand::Unbind => self.remove_calculation_binding(),
            CalculationCommand::Recalculate => self.recalculate_calculations(),
            CalculationCommand::Show => {
                self.mode = WorkspaceMode::TableEditor;
                self.status = "Table EditorのCalculation欄を表示しました".to_owned();
            }
        }
    }

    // {
    //   責務: [dispatch_calculation_command_id: stable IDから登録済みCalculation commandを解決して実行する。]
    //   引数: [command_id: 登録済みCalculation command ID。]
    //   戻り値: [(): 指定commandを実行し、未知IDはstatusへ表示する。]
    // }
    pub fn dispatch_calculation_command_id(&mut self, command_id: &str) {
        if let Some(command) = CalculationCommand::from_id(command_id) {
            self.dispatch_calculation_command(command);
        } else {
            self.status = format!("Unknown command: {command_id}");
        }
    }

    // {
    //   責務: [handle_calculation_shortcuts: Table Editorで計算規則登録のshortcutを処理する。]
    //   処理: [テキスト入力中の実行を避け、計算欄表示・再計算・削除のshortcutは共通Registryへ任せる。]
    //   引数: [self: GUI操作対象。 ctx: shortcutとkeyboard focusの参照元。]
    //   戻り値: [(): shortcutに対応するstable commandをdispatchする。]
    // }
    pub(super) fn handle_calculation_shortcuts(&mut self, ctx: &egui::Context) {
        if self.mode != WorkspaceMode::TableEditor || ctx.wants_keyboard_input() {
            return;
        }
        let bind =
            ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::Enter));
        if bind {
            self.dispatch_calculation_command(CalculationCommand::Bind);
        }
    }

    // {
    //   責務: [recalculate_calculations: 開いているdocumentの全Bindingを再評価する。]
    //   戻り値: [(): 結果またはerrorをstatusへ表示する。]
    // }
    fn recalculate_calculations(&mut self) {
        if let Some(document) = self.document.as_ref() {
            match self.calculation_engine.recalculate_all(document) {
                Ok(_) => self.status = "計算結果を更新しました".to_owned(),
                Err(error) => self.status = error.to_string(),
            }
        }
    }

    // {
    //   責務: [remove_calculation_binding: 選択cell/rangeに属するBindingを解除する。]
    //   戻り値: [(): 削除結果をstatusとproject config dirty stateへ反映する。]
    // }
    fn remove_calculation_binding(&mut self) {
        let selected_range = self.selected_calculation_range();
        let target = if self.selection == self.selection_anchor {
            CalculationTarget::Cell(self.selection)
        } else {
            CalculationTarget::Range(selected_range)
        };
        if self.guard_user_defined_calculation_target(target) {
            return;
        }
        let mut staged_engine = self.calculation_engine.clone();
        let result = selected_range
            .iter()
            .try_fold(false, |removed_any, target| {
                let binding_id = CalculationBindingId::new(format!("dsl:{target}"))
                    .map_err(|error| error.to_string())?;
                staged_engine
                    .remove_binding(&binding_id)
                    .map(|removed| removed_any || removed)
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok(true) => {
                self.calculation_engine = staged_engine;
                self.calculation_bindings_dirty = true;
                self.status = format!("{selected_range} の計算規則を削除しました");
            }
            Ok(false) => self.status = format!("{selected_range} に計算規則はありません"),
            Err(error) => self.status = error,
        }
    }

    // {
    //   責務: [apply_calculation_expression: 入力式を選択cell/rangeへ安全なDSL経由で適用する。]
    //   戻り値: [bool: Bindingの登録と再計算に成功した場合はtrue。]
    // }
    fn apply_calculation_expression(&mut self) -> bool {
        let trigger = match self.calculation_trigger {
            CalculationTrigger::DependencyChange => "DependencyChange",
            CalculationTrigger::Manual => "Manual",
        };
        let selected_range = self.selected_calculation_range();
        let target_kind = if self.selection == self.selection_anchor {
            "Cell"
        } else {
            "Range"
        };
        let calculation_target = if self.selection == self.selection_anchor {
            CalculationTarget::Cell(self.selection)
        } else {
            CalculationTarget::Range(selected_range)
        };
        if self.guard_user_defined_calculation_target(calculation_target) {
            return false;
        }
        let target_address = if target_kind == "Cell" {
            self.selection.to_string()
        } else {
            selected_range.to_string()
        };
        let source = format!(
            "SET This.Worksheet.Editor.{target_kind}(\"{target_address}\").Calculation.Expression = {}\nSET This.Worksheet.Editor.{target_kind}(\"{target_address}\").Calculation.Trigger = {}",
            self.calculation_expression, trigger
        );
        let program = match crate::rowly_dsl::parse(&source) {
            Ok(program) => program,
            Err(error) => {
                self.status = error.to_string();
                return false;
            }
        };
        let has_only_expected_binding_statements = matches!(
            program.statements(),
            [
                Statement::SetCalculationExpression {
                    target: expression_target,
                    ..
                },
                Statement::SetCalculationTrigger {
                    target: trigger_target,
                    trigger: parsed_trigger,
                },
            ] if expression_target == &calculation_target
                && trigger_target == &calculation_target
                && *parsed_trigger == self.calculation_trigger
        );
        if !program.classes().is_empty()
            || !program.functions().is_empty()
            || !has_only_expected_binding_statements
        {
            self.status = "式は単一のCalculation Bindingとして入力してください".to_owned();
            return false;
        }
        let Some(document) = self.document.as_mut() else {
            return false;
        };
        let mut staged_engine = self.calculation_engine.clone();
        match crate::rowly_dsl::execute_with_calculation_engine(
            &program,
            document,
            &mut staged_engine,
        ) {
            Ok(_) => {
                self.calculation_engine = staged_engine;
                self.calculation_bindings_dirty = true;
                match self.calculation_engine.recalculate_all(document) {
                    Ok(_) => self.status = format!("{selected_range} の計算規則を適用しました"),
                    Err(error) => self.status = error.to_string(),
                }
                true
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    // {
    //   責務: [selected_calculation_range: anchorとselectionを含む矩形rangeを返す。]
    //   戻り値: [CellRange: selectionの向きに依存しない正規化range。]
    // }
    fn selected_calculation_range(&self) -> CellRange {
        CellRange::new(self.selection_anchor, self.selection)
    }

    fn guard_user_defined_calculation_target(&mut self, target: CalculationTarget) -> bool {
        let result = match (&self.project_session, self.project_source_id.as_deref()) {
            (Some(session), Some(source_id)) => {
                session.user_defines_calculation_binding(source_id, target)
            }
            _ => return false,
        };
        match result {
            Ok(false) => false,
            Ok(true) => {
                self.status =
                    "この計算規則はuser.rlyで管理されています。user.rlyを編集してください。"
                        .to_owned();
                true
            }
            Err(error) => {
                self.status = error.to_string();
                true
            }
        }
    }
}
