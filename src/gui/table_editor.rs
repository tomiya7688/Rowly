use eframe::egui;

use super::{
    CELL_WIDTH, GUTTER_WIDTH, ROW_HEIGHT, RowlyApp, WorkspaceMode, cell_range, column_label,
    update_selection,
};
use crate::process::CellRef;

impl RowlyApp {
    // {
    //   責務: [show_editor_grid: CSVのrow・column・cellをvirtualized table gridとして描画し編集操作を処理する。]
    //   処理: [visible row/columnを描画し、selection・inline edit・allowed value候補・cell commitを更新する。]
    //   引数: [self: document・selection・editing state。 ui: gridを描画する領域。]
    //   戻り値: [(): table表示と入力操作を反映する。]
    //   副作用: [確定操作時にprocess経由でcellを変更する。]
    // }
    pub(super) fn show_editor_grid(&mut self, ui: &mut egui::Ui) {
        let document = self.document.as_ref().expect("document checked above");
        let row_count = document.row_count();
        let column_count = document.column_count();
        let total_width = GUTTER_WIDTH + column_count as f32 * CELL_WIDTH;
        let allowed_values = document
            .allowed_values_for_column(self.selection.column())
            .map(<[String]>::to_vec);
        let mut commit_value = None;
        let mut candidate_popup_anchor = None;
        if !self.editing {
            self.validation_candidates_open = false;
        }

        if row_count == 0 || column_count == 0 {
            ui.label("CSVにセルがありません。行や列を挿入して編集を開始できます。");
        }

        egui::ScrollArea::horizontal()
            .id_salt("table_grid_horizontal")
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                ui.set_min_width(total_width.max(viewport.width()));
                let first_column = (((viewport.min.x - GUTTER_WIDTH).max(0.0) / CELL_WIDTH).floor()
                    as usize)
                    .min(column_count);
                let last_column = (((viewport.max.x - GUTTER_WIDTH).max(0.0) / CELL_WIDTH).ceil()
                    as usize)
                    .saturating_add(1)
                    .min(column_count);

                ui.horizontal(|ui| {
                    ui.add_sized([GUTTER_WIDTH, ROW_HEIGHT], egui::Label::new("#"));
                    ui.add_space(first_column as f32 * CELL_WIDTH);
                    for column in first_column..last_column {
                        let label = column_label(column);
                        let selected = self.selection.column() == column;
                        if ui
                            .add_sized(
                                [CELL_WIDTH, ROW_HEIGHT],
                                egui::Button::new(label).selected(selected),
                            )
                            .clicked()
                        {
                            let row = self.selection.row();
                            update_selection(
                                &mut self.selection,
                                &mut self.selection_anchor,
                                &mut self.reference_input,
                                &mut self.editing,
                                CellRef::new(row, column),
                                false,
                                (row_count, column_count),
                            );
                        }
                    }
                });

                egui::ScrollArea::vertical()
                    .id_salt("table_grid_vertical")
                    .auto_shrink([false, false])
                    .show_rows(ui, ROW_HEIGHT, row_count, |ui, visible_rows| {
                        for row in visible_rows {
                            ui.horizontal(|ui| {
                                let row_header = ui.add_sized(
                                    [GUTTER_WIDTH, ROW_HEIGHT],
                                    egui::Button::new((row + 1).to_string())
                                        .selected(self.selection.row() == row),
                                );
                                if row_header.clicked() {
                                    let column = self.selection.column();
                                    update_selection(
                                        &mut self.selection,
                                        &mut self.selection_anchor,
                                        &mut self.reference_input,
                                        &mut self.editing,
                                        CellRef::new(row, column),
                                        false,
                                        (row_count, column_count),
                                    );
                                }
                                ui.add_space(first_column as f32 * CELL_WIDTH);

                                for column in first_column..last_column {
                                    let reference = CellRef::new(row, column);
                                    let value = document.cell(row, column);
                                    let range = cell_range(self.selection_anchor, self.selection);
                                    let selected = row >= range.0
                                        && row <= range.1
                                        && column >= range.2
                                        && column <= range.3;

                                    if self.editing
                                        && self.selection == reference
                                        && value.is_some()
                                    {
                                        let candidates = if row > 0 {
                                            allowed_values.as_deref()
                                        } else {
                                            None
                                        };
                                        let has_candidate_rule = candidates.is_some();
                                        let mut open_clicked = false;
                                        let editor_width = if has_candidate_rule {
                                            CELL_WIDTH - 24.0
                                        } else {
                                            CELL_WIDTH
                                        };
                                        ui.horizontal(|ui| {
                                            let response = ui.add_sized(
                                                [editor_width, ROW_HEIGHT],
                                                egui::TextEdit::singleline(&mut self.edit_value),
                                            );
                                            response.request_focus();
                                            candidate_popup_anchor = Some(response.rect);
                                            if has_candidate_rule
                                                && ui
                                                    .add_sized(
                                                        [24.0, ROW_HEIGHT],
                                                        egui::Button::new("▾"),
                                                    )
                                                    .clicked()
                                            {
                                                open_clicked = true;
                                            }
                                        });

                                        let (alt_down, down, up, enter, escape, tab) =
                                            ui.input(|input| {
                                                (
                                                    input.modifiers.alt
                                                        && input.key_pressed(egui::Key::ArrowDown),
                                                    input.key_pressed(egui::Key::ArrowDown),
                                                    input.key_pressed(egui::Key::ArrowUp),
                                                    input.key_pressed(egui::Key::Enter),
                                                    input.key_pressed(egui::Key::Escape),
                                                    input.key_pressed(egui::Key::Tab),
                                                )
                                            });
                                        if open_clicked || has_candidate_rule && alt_down {
                                            self.validation_candidates_open = true;
                                        } else if self.validation_candidates_open
                                            && has_candidate_rule
                                        {
                                            let candidates = candidates.unwrap_or_default();
                                            if down {
                                                self.validation_candidate_index = self
                                                    .validation_candidate_index
                                                    .saturating_add(1)
                                                    .min(candidates.len().saturating_sub(1));
                                            } else if up {
                                                self.validation_candidate_index = self
                                                    .validation_candidate_index
                                                    .saturating_sub(1);
                                            }
                                            if enter {
                                                if let Some(candidate) =
                                                    candidates.get(self.validation_candidate_index)
                                                {
                                                    commit_value = Some((
                                                        row,
                                                        column,
                                                        candidate.clone(),
                                                        false,
                                                    ));
                                                    self.editing = false;
                                                    self.validation_candidates_open = false;
                                                }
                                            } else if tab {
                                                let selected = candidates
                                                    .get(self.validation_candidate_index)
                                                    .cloned()
                                                    .unwrap_or_else(|| self.edit_value.clone());
                                                commit_value = Some((row, column, selected, true));
                                                self.editing = false;
                                                self.validation_candidates_open = false;
                                            } else if escape {
                                                self.validation_candidates_open = false;
                                            }
                                        } else if enter || tab {
                                            commit_value =
                                                Some((row, column, self.edit_value.clone(), tab));
                                            self.editing = false;
                                            self.validation_candidates_open = false;
                                        } else if escape {
                                            self.editing = false;
                                            self.validation_candidates_open = false;
                                        }
                                    } else {
                                        if let Some(value) = value {
                                            let response = ui.add_sized(
                                                [CELL_WIDTH, ROW_HEIGHT],
                                                egui::Button::new(value).selected(selected),
                                            );
                                            if response.clicked() {
                                                let extend =
                                                    ui.input(|input| input.modifiers.shift);
                                                update_selection(
                                                    &mut self.selection,
                                                    &mut self.selection_anchor,
                                                    &mut self.reference_input,
                                                    &mut self.editing,
                                                    reference,
                                                    extend,
                                                    (row_count, column_count),
                                                );
                                            }
                                            if response.double_clicked() {
                                                update_selection(
                                                    &mut self.selection,
                                                    &mut self.selection_anchor,
                                                    &mut self.reference_input,
                                                    &mut self.editing,
                                                    reference,
                                                    false,
                                                    (row_count, column_count),
                                                );
                                                self.edit_value = value.to_owned();
                                                self.editing = true;
                                                self.validation_candidates_open = false;
                                                self.validation_candidate_index = document
                                                    .allowed_values_for_column(column)
                                                    .and_then(|values| {
                                                        values.iter().position(|candidate| {
                                                            candidate == value
                                                        })
                                                    })
                                                    .unwrap_or(0);
                                            }
                                        } else {
                                            ui.add_enabled_ui(false, |ui| {
                                                ui.add_sized(
                                                    [CELL_WIDTH, ROW_HEIGHT],
                                                    egui::Button::new(""),
                                                );
                                            });
                                        }
                                    }
                                }
                            });
                        }
                    });
            });

        if self.editing && self.validation_candidates_open {
            if let (Some(anchor), Some(candidates)) = (candidate_popup_anchor, allowed_values) {
                let reference = self.selection;
                let context = ui.ctx().clone();
                egui::Area::new(egui::Id::new((
                    "validation-candidates",
                    reference.row(),
                    reference.column(),
                )))
                .order(egui::Order::Foreground)
                .fixed_pos(anchor.left_bottom())
                .show(&context, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(anchor.width());
                        if candidates.is_empty() {
                            ui.weak("この列には許可値がありません");
                        } else {
                            egui::ScrollArea::vertical()
                                .id_salt(("validation-candidate-list", reference))
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    for (index, candidate) in candidates.iter().enumerate() {
                                        let selected = index == self.validation_candidate_index;
                                        if ui.selectable_label(selected, candidate).clicked() {
                                            commit_value = Some((
                                                reference.row(),
                                                reference.column(),
                                                candidate.clone(),
                                                false,
                                            ));
                                            self.validation_candidate_index = index;
                                            self.editing = false;
                                            self.validation_candidates_open = false;
                                        }
                                    }
                                });
                        }
                    });
                });
            }
        }

        if let Some((row, column, value, advance_after_tab)) = commit_value {
            if self.set_cell(row, column, value.clone()) {
                if advance_after_tab {
                    self.move_selection_after_tab();
                }
            } else {
                self.editing = true;
                self.edit_value = value;
            }
        }
    }

    // {
    //   責務: [handle_grid_keys: grid操作中のkeyboard shortcutをselection・edit・undo/redoへ適用する。]
    //   処理: [text inputへfocus中・grid外・inline edit中を除外し、navigationとcommand keyをdispatchする。]
    //   引数: [self: mode・selection・document・editing状態。 ctx: key inputとfocus状態を読むegui context。]
    //   戻り値: [(): keyboard inputに応じた画面操作を実行する。]
    // }
    pub(super) fn handle_grid_keys(&mut self, ctx: &egui::Context) {
        if self.mode != WorkspaceMode::TableEditor || ctx.wants_keyboard_input() {
            return;
        }
        if self.editing {
            return;
        }

        let (undo, redo, edit, cancel, direction, extend, copy, paste_text) = ctx.input(|input| {
            let command = input.modifiers.command;
            let direction = if input.key_pressed(egui::Key::ArrowUp) {
                Some((0isize, -1isize))
            } else if input.key_pressed(egui::Key::ArrowDown) {
                Some((0, 1))
            } else if input.key_pressed(egui::Key::ArrowLeft) {
                Some((-1, 0))
            } else if input.key_pressed(egui::Key::ArrowRight) {
                Some((1, 0))
            } else {
                None
            };
            (
                command && input.key_pressed(egui::Key::Z) && !input.modifiers.shift,
                command
                    && (input.key_pressed(egui::Key::Y)
                        || input.modifiers.shift && input.key_pressed(egui::Key::Z)),
                input.key_pressed(egui::Key::F2) || input.key_pressed(egui::Key::Enter),
                input.key_pressed(egui::Key::Escape),
                direction,
                input.modifiers.shift,
                command && input.key_pressed(egui::Key::C),
                input.events.iter().find_map(|event| match event {
                    egui::Event::Paste(text) => Some(text.clone()),
                    _ => None,
                }),
            )
        });

        if copy {
            self.copy_selected_csv_values(ctx);
        } else if let Some(text) = paste_text {
            self.paste_csv_values(&text);
        } else if undo {
            self.undo();
        } else if redo {
            self.redo();
        } else if cancel {
            self.editing = false;
        } else if let Some((dx, dy)) = direction {
            let next = CellRef::new(
                self.selection.row().saturating_add_signed(dy),
                self.selection.column().saturating_add_signed(dx),
            );
            self.set_selection(next, extend);
        } else if edit {
            let value = self
                .document
                .as_ref()
                .and_then(|document| document.cell(self.selection.row(), self.selection.column()))
                .map(str::to_owned);
            if let Some(value) = value {
                self.begin_edit(&value);
            } else {
                self.status = format!("{} is outside this CSV row", self.selection);
            }
        }
    }
}
