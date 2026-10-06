use eframe::egui;
use std::path::PathBuf;

use crate::process::{CellRef, CsvDocument, CsvFileWatcher, DocumentError, ValidationViolation};

#[path = "gui_help.rs"]
mod help;
pub use help::{HelpDocument, HelpTarget};

// {
//   責務: [CELL_WIDTH: grid cell・column headerの表示幅をpixel単位で定義する。]
// }
const CELL_WIDTH: f32 = 120.0;
// {
//   責務: [ROW_HEIGHT: grid row・headerの表示高をpixel単位で定義する。]
// }
const ROW_HEIGHT: f32 = 26.0;
// {
//   責務: [GUTTER_WIDTH: row number gutterの表示幅をpixel単位で定義する。]
// }
const GUTTER_WIDTH: f32 = 56.0;

// {
//   責務: [WorkspaceMode: RowlyAppが表示する編集・閲覧画面を識別する。]
//   選択肢: [TableEditor: セルと行列を編集。 TextEditor: CSVテキストを編集。 Viewer: 表を閲覧。 Help: ヘルプを表示。]
// }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum WorkspaceMode {
    #[default]
    TableEditor,
    TextEditor,
    Viewer,
    Help,
}

// {
//   責務: [RowlyApp: CSV documentとGUI各画面の状態を保持し、ユーザー操作をprocess APIへ仲介する。]
//   フィールド: [mode/help_return_mode: 現在画面とhelpからの復帰先。 path_input/reference_input: pathとcell address入力。 code_buffer/code_buffer_dirty: CSV text編集値と未適用状態。 document/file_watcher: 開いたCSVと外部変更監視。 status/zoom: 表示messageと拡大率。 selection/selection_anchor: 選択cellと範囲起点。 editing/edit_value: inline edit状態と入力text。 validation_candidates_open/validation_candidate_index: 入力候補popup状態。 help: help viewer。]
// }
pub struct RowlyApp {
    mode: WorkspaceMode,
    help_return_mode: WorkspaceMode,
    path_input: String,
    reference_input: String,
    code_buffer: String,
    code_buffer_dirty: bool,
    document: Option<CsvDocument>,
    file_watcher: Option<CsvFileWatcher>,
    status: String,
    zoom: f32,
    selection: CellRef,
    selection_anchor: CellRef,
    editing: bool,
    edit_value: String,
    validation_candidates_open: bool,
    validation_candidate_index: usize,
    help: help::HelpViewer,
}

impl Default for RowlyApp {
    // {
    //   責務: [default: 初期画面に必要なGUI状態を作る。]
    //   処理: [table editor、空入力、A1選択、既定zoom、空documentとhelp viewerを設定する。]
    //   引数: []
    //   戻り値: [RowlyApp: CSV未読込の初期GUI状態。]
    // }
    fn default() -> Self {
        Self {
            mode: WorkspaceMode::TableEditor,
            help_return_mode: WorkspaceMode::TableEditor,
            path_input: String::new(),
            reference_input: "A1".to_owned(),
            code_buffer: String::new(),
            code_buffer_dirty: false,
            document: None,
            file_watcher: None,
            status: "CSV ファイルを開いてください".to_owned(),
            zoom: 100.0,
            selection: CellRef::new(0, 0),
            selection_anchor: CellRef::new(0, 0),
            editing: false,
            edit_value: String::new(),
            validation_candidates_open: false,
            validation_candidate_index: 0,
            help: help::HelpViewer::default(),
        }
    }
}

impl RowlyApp {
    // {
    //   責務: [open_csv: 入力pathのCSVを開き、documentと関連するGUI状態を初期化する。]
    //   処理: [CsvDocumentを開き、text bufferとfile watcherを準備し、selectionとstatusを更新する。]
    //   引数: [self: 更新対象の画面状態。 ctx: file change時にrepaintするegui context。]
    //   戻り値: [(): 成功・失敗をstatusへ反映する。]
    //   エラー: [open・CSV text取得・file watcher開始の失敗をstatusへ表示する。]
    // }
    fn open_csv(&mut self, ctx: &egui::Context) {
        let path = PathBuf::from(self.path_input.trim());
        match CsvDocument::open(&path) {
            Ok(document) => {
                let code_buffer = match document.csv_text() {
                    Ok(text) => text,
                    Err(error) => {
                        self.status = error.to_string();
                        return;
                    }
                };
                let repaint_context = ctx.clone();
                let watcher = CsvFileWatcher::new(document.path(), move || {
                    repaint_context.request_repaint();
                });
                self.status = match &watcher {
                    Ok(_) => format!("{} を開きました", path.display()),
                    Err(error) => format!(
                        "{} を開きました（外部変更の監視を開始できません: {}; 保存時の確認は有効です）",
                        path.display(),
                        error
                    ),
                };
                self.path_input = path.display().to_string();
                self.code_buffer = code_buffer;
                self.code_buffer_dirty = false;
                self.document = Some(document);
                self.file_watcher = watcher.ok();
                self.selection = CellRef::new(0, 0);
                self.selection_anchor = self.selection;
                self.reference_input = self.selection.to_string();
                self.editing = false;
                self.validation_candidates_open = false;
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    // {
    //   責務: [save_csv: 開いているdocumentを保存し結果をstatusへ表示する。]
    //   処理: [CsvDocument::saveを呼び、保存先またはerrorをstatusへ設定する。]
    //   引数: [self: 保存対象documentを保持する画面状態。]
    //   戻り値: [(): 保存成否をstatusへ反映する。]
    // }
    fn save_csv(&mut self) {
        let Some(document) = self.document.as_mut() else {
            self.status = "保存する CSV がありません".to_owned();
            return;
        };
        match document.save() {
            Ok(()) => self.status = format!("{} を保存しました", document.path().display()),
            Err(error) => self.status = error.to_string(),
        }
    }

    // {
    //   責務: [show_menu: file・edit・help menuを描画し、選択操作を実行する。]
    //   処理: [menu actionを記録してUI描画後にopen・save・undo・redo・help遷移を呼ぶ。]
    //   引数: [self: 表示状態と操作対象document。 ctx: menu描画先。]
    //   戻り値: [(): menu表示と選択された操作を反映する。]
    // }
    fn show_menu(&mut self, ctx: &egui::Context) {
        let mut open_requested = false;
        let mut save_requested = false;
        let mut help_target = None;
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open CSV…").clicked() {
                        open_requested = true;
                        ui.close_menu();
                    }
                    if ui.button("Save").clicked() {
                        save_requested = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    let can_undo = self.document.as_ref().is_some_and(CsvDocument::can_undo);
                    let can_redo = self.document.as_ref().is_some_and(CsvDocument::can_redo);
                    if ui
                        .add_enabled(can_undo, egui::Button::new("Undo"))
                        .clicked()
                    {
                        self.undo();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(can_redo, egui::Button::new("Redo"))
                        .clicked()
                    {
                        self.redo();
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.label("Choose a workspace mode from the toolbar.");
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("DSL 文法リファレンス").clicked() {
                        help_target = Some(help::HelpTarget::new(
                            help::HelpDocument::Reference,
                            "source",
                        ));
                        ui.close_menu();
                    }
                    if ui.button("DSL チュートリアル").clicked() {
                        help_target = Some(help::HelpTarget::new(
                            help::HelpDocument::Tutorial,
                            "tutorial-getting-started",
                        ));
                        ui.close_menu();
                    }
                });
                ui.separator();
                ui.label("Rowly");
            });
        });
        if open_requested {
            self.open_csv(ctx);
        }
        if save_requested {
            self.save_csv();
        }
        if let Some(target) = help_target {
            self.open_help_target(target);
        }
    }

    // {
    //   責務: [show_toolbar: path・cell reference・編集操作・workspace切替用toolbarを描画する。]
    //   処理: [document状態に応じた操作を有効化し、UI描画後に選択された操作を実行する。]
    //   引数: [self: 入力値・workspace・documentを保持する画面状態。 ctx: toolbar描画先。]
    //   戻り値: [(): toolbar表示とユーザー操作を反映する。]
    // }
    fn show_toolbar(&mut self, ctx: &egui::Context) {
        if self.mode == WorkspaceMode::Help {
            return;
        }
        let mut open_requested = false;
        let mut save_requested = false;
        let mut undo_requested = false;
        let mut redo_requested = false;
        let mut insert_row_requested = false;
        let mut delete_row_requested = false;
        let mut insert_column_requested = false;
        let mut delete_column_requested = false;
        let mut navigate_requested = false;
        let mut apply_code_requested = false;
        let previous_mode = self.mode;
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("CSV");
                ui.add(
                    egui::TextEdit::singleline(&mut self.path_input)
                        .hint_text("CSV file path")
                        .desired_width(360.0),
                );
                if ui.button("Open").clicked() {
                    open_requested = true;
                }
                if ui
                    .add_enabled(
                        self.document.is_some()
                            && !(self.mode == WorkspaceMode::TextEditor && self.code_buffer_dirty),
                        egui::Button::new("Save"),
                    )
                    .clicked()
                {
                    save_requested = true;
                }
                if self.mode == WorkspaceMode::TextEditor
                    && ui
                        .add_enabled(self.document.is_some(), egui::Button::new("Apply Code"))
                        .clicked()
                {
                    apply_code_requested = true;
                }
                let can_undo = self.document.as_ref().is_some_and(CsvDocument::can_undo);
                let can_redo = self.document.as_ref().is_some_and(CsvDocument::can_redo);
                if ui
                    .add_enabled(can_undo, egui::Button::new("Undo"))
                    .clicked()
                {
                    undo_requested = true;
                }
                if ui
                    .add_enabled(can_redo, egui::Button::new("Redo"))
                    .clicked()
                {
                    redo_requested = true;
                }
                ui.separator();
                ui.label("Cell");
                let address = ui
                    .add(egui::TextEdit::singleline(&mut self.reference_input).desired_width(58.0));
                if address.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    navigate_requested = true;
                }
                if ui.button("Go").clicked() {
                    navigate_requested = true;
                }
                if self.mode == WorkspaceMode::TableEditor {
                    ui.separator();
                    if ui.button("Insert row").clicked() {
                        insert_row_requested = true;
                    }
                    if ui.button("Delete row").clicked() {
                        delete_row_requested = true;
                    }
                    if ui.button("Insert column").clicked() {
                        insert_column_requested = true;
                    }
                    if ui.button("Delete column").clicked() {
                        delete_column_requested = true;
                    }
                }
                ui.separator();
                ui.selectable_value(&mut self.mode, WorkspaceMode::TableEditor, "Table Editor");
                ui.selectable_value(&mut self.mode, WorkspaceMode::TextEditor, "Text Editor");
                ui.selectable_value(&mut self.mode, WorkspaceMode::Viewer, "Viewer");
            });
        });
        if open_requested {
            self.open_csv(ctx);
        }
        if save_requested {
            self.save_csv();
        }
        if undo_requested {
            self.undo();
        }
        if redo_requested {
            self.redo();
        }
        if insert_row_requested {
            self.insert_row();
        }
        if delete_row_requested {
            self.delete_row();
        }
        if insert_column_requested {
            self.insert_column();
        }
        if delete_column_requested {
            self.delete_column();
        }
        if navigate_requested {
            self.navigate_to_cell();
        }
        if previous_mode != WorkspaceMode::TextEditor
            && self.mode == WorkspaceMode::TextEditor
            && !self.code_buffer_dirty
        {
            self.refresh_code_buffer();
        }
        if apply_code_requested {
            self.apply_code_buffer();
        }
    }

    // {
    //   責務: [show_workspace: 選択中modeに対応するhelp・table・text・viewer画面を描画する。]
    //   処理: [document未読込表示、mode別widget、text buffer変更状態を更新する。]
    //   引数: [self: workspace・document・buffer状態。 ctx: workspace描画先。]
    //   戻り値: [(): 選択modeの画面を描画する。]
    // }
    fn show_workspace(&mut self, ctx: &egui::Context) {
        if self.mode == WorkspaceMode::Help {
            if self.help.show(ctx) {
                self.mode = self.help_return_mode;
            }
            return;
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.document.is_none() {
                ui.vertical_centered(|ui| {
                    ui.add_space(120.0);
                    ui.heading("Rowly");
                    ui.label("CSV を開くと、ここにワークスペースが表示されます。");
                });
                return;
            }

            match self.mode {
                WorkspaceMode::TableEditor => {
                    ui.heading("Table Editor");
                    self.show_editor_grid(ui);
                }
                WorkspaceMode::TextEditor => {
                    ui.heading("Text Editor");
                    ui.label("CSVテキストを編集し、Apply Codeで入力規則を確認して反映します。");
                    if self.code_buffer_dirty {
                        ui.weak("未適用のコード変更があります");
                    }
                    let mut changed = false;
                    egui::ScrollArea::both().show(ui, |ui| {
                        changed = ui
                            .add(
                                egui::TextEdit::multiline(&mut self.code_buffer)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(20),
                            )
                            .changed();
                    });
                    if changed {
                        self.code_buffer_dirty = true;
                    }
                }
                WorkspaceMode::Viewer => {
                    let document = self.document.as_ref().expect("document checked above");
                    ui.heading("Viewer");
                    self.show_table_preview(ui, document, true);
                }
                WorkspaceMode::Help => unreachable!("handled before workspace rendering"),
            }
        });
    }

    /// {
    ///   責務: [open_help_target: 指定されたhelp targetを開き、元画面への復帰先を保持する。]
    ///   処理: [初回遷移時のmodeを保存してhelp viewerを開き、modeをHelpにする。]
    ///   引数: [self: modeとhelp viewerを持つ画面状態。 target: 表示するhelp文書とanchor。]
    ///   戻り値: [(): help画面へ遷移する。]
    /// }
    pub fn open_help_target(&mut self, target: help::HelpTarget) {
        if self.mode != WorkspaceMode::Help {
            self.help_return_mode = self.mode;
        }
        self.help.open(target);
        self.mode = WorkspaceMode::Help;
    }

    // {
    //   責務: [show_external_conflicts: 外部変更とのcell・row構造競合を表示し、選択されたlocal draftを再適用する。]
    //   処理: [競合baseline/local/disk情報を描画し、明示操作時のみCsvDocumentの再適用APIを呼ぶ。]
    //   引数: [self: 競合draftを保持するdocumentとstatus。 ctx: 競合表示領域。]
    //   戻り値: [(): 競合内容を表示し、再適用結果をstatusへ反映する。]
    //   副作用: [選択操作に応じてdocumentのlocal競合値または表全体を再適用する。]
    // }
    fn show_external_conflicts(&mut self, ctx: &egui::Context) {
        let draft_count = self
            .document
            .as_ref()
            .map_or(0, |document| document.external_conflict_drafts().len());
        if draft_count == 0 {
            return;
        }

        let mut reapply_draft = None;
        egui::TopBottomPanel::top("external_conflicts").show(ctx, |ui| {
            ui.heading(format!("外部変更の競合 ({draft_count})"));
            egui::ScrollArea::vertical()
                .id_salt("external_conflict_list")
                .max_height(190.0)
                .show(ui, |ui| {
                    if let Some(document) = self.document.as_ref() {
                        for (draft_index, draft) in
                            document.external_conflict_drafts().iter().enumerate()
                        {
                            egui::CollapsingHeader::new(format!("競合 {}", draft_index + 1))
                                .id_salt(draft_index)
                                .default_open(true)
                                .show(ui, |ui| {
                                    if let Some(reason) = draft.structural_conflict {
                                        ui.label(format!(
                                            "行構造の競合: {reason:?}。Disk を現在値として使っています。"
                                        ));
                                        ui.label(format!(
                                            "Baseline {} 行 / Local {} 行 / Disk {} 行",
                                            draft.baseline.len(),
                                            draft.local.len(),
                                            draft.disk.len()
                                        ));
                                        egui::ScrollArea::vertical()
                                            .id_salt(("structural_diff", draft_index))
                                            .max_height(100.0)
                                            .show(ui, |ui| {
                                                let row_count = draft
                                                    .baseline
                                                    .len()
                                                    .max(draft.local.len())
                                                    .max(draft.disk.len());
                                                for row in 0..row_count {
                                                    let baseline = draft.baseline.get(row);
                                                    let local = draft.local.get(row);
                                                    let disk = draft.disk.get(row);
                                                    if baseline != local || baseline != disk {
                                                        ui.monospace(format!(
                                                            "行 {}: B={baseline:?}  L={local:?}  D={disk:?}",
                                                            row + 1
                                                        ));
                                                    }
                                                }
                                            });
                                        if ui.button("Local の表全体を再適用").clicked() {
                                            reapply_draft = Some((draft_index, true));
                                        }
                                    }
                                    for conflict in &draft.cell_conflicts {
                                        let cell = CellRef::new(conflict.row, conflict.column);
                                        ui.monospace(format!(
                                            "{cell}: B={:?}  L={:?}  D={:?}",
                                            conflict.baseline, conflict.local, conflict.disk
                                        ));
                                    }
                                    if !draft.cell_conflicts.is_empty()
                                        && ui.button("Local の競合値を再適用").clicked()
                                    {
                                        reapply_draft = Some((draft_index, false));
                                    }
                                });
                        }
                    }
                });
        });

        if let Some((draft_index, structural)) = reapply_draft {
            let document = self
                .document
                .as_mut()
                .expect("conflict panel requires an open document");
            self.status = if structural {
                match document.reapply_local_structural_draft(draft_index) {
                    Ok(count) => format!("Local の表を {count} 行で再適用しました"),
                    Err(error) => error.to_string(),
                }
            } else {
                match document.reapply_local_cell_conflicts(draft_index) {
                    Ok(count) => format!("Local の競合値を {count} セル再適用しました"),
                    Err(error) => error.to_string(),
                }
            };
        }
    }

    // {
    //   責務: [show_editor_grid: CSVのrow・column・cellをvirtualized table gridとして描画し編集操作を処理する。]
    //   処理: [visible row/columnを描画し、selection・inline edit・allowed value候補・cell commitを更新する。]
    //   引数: [self: document・selection・editing state。 ui: gridを描画する領域。]
    //   戻り値: [(): table表示と入力操作を反映する。]
    //   副作用: [確定操作時にprocess経由でcellを変更する。]
    // }
    fn show_editor_grid(&mut self, ui: &mut egui::Ui) {
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
    //   責務: [refresh_code_buffer: CSV documentの現在のtextをeditor bufferへ同期する。]
    //   処理: [documentがなければbufferをclearし、取得できればtextを置き換えてdirty flagを解除する。]
    //   引数: [self: document・code buffer・statusを保持する画面状態。]
    //   戻り値: [(): text取得errorはstatusへ反映する。]
    // }
    fn refresh_code_buffer(&mut self) {
        let Some(document) = self.document.as_ref() else {
            self.code_buffer.clear();
            self.code_buffer_dirty = false;
            return;
        };
        match document.csv_text() {
            Ok(text) => {
                self.code_buffer = text;
                self.code_buffer_dirty = false;
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    // {
    //   責務: [apply_code_buffer: text editorのCSV textをdocumentへ検証付きで適用する。]
    //   処理: [CsvDocument::apply_csv_textを呼び、成功時にbuffer・selectionを同期し、失敗理由をstatusへ示す。]
    //   引数: [self: 適用対象documentとcode buffer。]
    //   戻り値: [(): 適用成否をdirty flag・buffer・statusへ反映する。]
    // }
    fn apply_code_buffer(&mut self) {
        let Some(document) = self.document.as_mut() else {
            self.status = "適用する CSV がありません".to_owned();
            return;
        };
        let result = document.apply_csv_text(&self.code_buffer);
        match result {
            Ok(()) => {
                self.code_buffer_dirty = false;
                self.status = "Code changes applied".to_owned();
                self.refresh_code_buffer();
                self.clamp_selection();
            }
            Err(DocumentError::ValidationRejected { violations }) => {
                self.status = format_validation_violations(&violations);
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    // {
    //   責務: [set_selection: document bounds内へcell selectionを更新する。]
    //   処理: [現在のrow・column数を取得しupdate_selectionへselectionとanchor更新を委譲する。]
    //   引数: [self: selection・anchor・reference入力を保持する画面状態。 reference: 選択先。 extend: anchorを維持するか。]
    //   戻り値: [(): selection・address入力・editing状態を同期する。]
    // }
    fn set_selection(&mut self, reference: CellRef, extend: bool) {
        let (row_count, column_count) = self.document.as_ref().map_or((0, 0), |document| {
            (document.row_count(), document.column_count())
        });
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

    // {
    //   責務: [begin_edit: 選択cellのinline editを開始し、既存値に対応する候補位置を設定する。]
    //   処理: [入力値をcopyし、allowed values内で一致する候補indexを探す。]
    //   引数: [self: selection・editing・candidate state。 value: 編集開始時のcell text。]
    //   戻り値: [(): inline editを有効化する。]
    // }
    fn begin_edit(&mut self, value: &str) {
        self.edit_value = value.to_owned();
        self.editing = true;
        self.validation_candidates_open = false;
        self.validation_candidate_index = self
            .document
            .as_ref()
            .and_then(|document| {
                document
                    .allowed_values_for_column(self.selection.column())
                    .and_then(|values| values.iter().position(|candidate| candidate == value))
            })
            .unwrap_or(0);
    }

    // {
    //   責務: [set_cell: process APIでcell textを更新し結果をstatusへ反映する。]
    //   処理: [documentがあればset_cellを呼び、入力規則違反と他のdocument errorを区別する。]
    //   引数: [self: 更新対象documentとstatus。 row: 0-based row index。 column: 0-based column index。 value: 書き込むtext。]
    //   戻り値: [bool: 更新成功ならtrue、document不在または失敗ならfalse。]
    //   副作用: [成功時にdocumentを編集する。]
    // }
    fn set_cell(&mut self, row: usize, column: usize, value: String) -> bool {
        let Some(document) = self.document.as_mut() else {
            return false;
        };
        match document.set_cell(row, column, value) {
            Ok(()) => {
                self.status = format!("Cell updated ({})", self.selection);
                true
            }
            Err(DocumentError::ValidationRejected { violations }) => {
                self.status = format_validation_violations(&violations);
                false
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    // {
    //   責務: [move_selection_after_tab: Tab確定後の次cellへselectionを進める。]
    //   処理: [同じrowの次columnを優先し、末columnなら次rowの先頭columnへ移動する。]
    //   引数: [self: documentと現在selectionを保持する画面状態。]
    //   戻り値: [(): selection更新対象がある場合に次cellへ移動する。]
    // }
    fn move_selection_after_tab(&mut self) {
        let Some(document) = self.document.as_ref() else {
            return;
        };
        let row_count = document.row_count();
        let column_count = document.column_count();
        if row_count == 0 || column_count == 0 {
            return;
        }

        let next = if self.selection.column() + 1 < column_count {
            CellRef::new(self.selection.row(), self.selection.column() + 1)
        } else if self.selection.row() + 1 < row_count {
            CellRef::new(self.selection.row() + 1, 0)
        } else {
            self.selection
        };
        self.set_selection(next, false);
    }

    // {
    //   責務: [navigate_to_cell: reference入力をCellRefとして解析し、該当cellへ移動する。]
    //   処理: [入力をparseし成功時はselection/statusを更新し、parse errorはstatusへ表示する。]
    //   引数: [self: reference入力・selection・statusを保持する画面状態。]
    //   戻り値: [(): reference解析結果を画面状態へ反映する。]
    // }
    fn navigate_to_cell(&mut self) {
        match self.reference_input.parse::<CellRef>() {
            Ok(reference) => {
                self.set_selection(reference, false);
                self.status = format!("Selected {}", self.selection);
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    // {
    //   責務: [insert_row: 選択rowの位置へ1行を挿入する。]
    //   処理: [CsvDocument::insert_rowsを呼び、共通結果処理へ渡す。]
    //   引数: [self: 選択位置・documentを保持する画面状態。]
    //   戻り値: [(): 挿入結果をstatus・selectionへ反映する。]
    //   副作用: [documentがあればrow構造を変更する。]
    // }
    fn insert_row(&mut self) {
        let result = self
            .document
            .as_mut()
            .map(|document| document.insert_rows(self.selection.row(), 1));
        self.apply_structure_result(result, "Inserted row");
    }

    // {
    //   責務: [delete_row: 選択rowを1行削除する。]
    //   処理: [CsvDocument::delete_rowsを呼び、共通結果処理へ渡す。]
    //   引数: [self: 選択位置・documentを保持する画面状態。]
    //   戻り値: [(): 削除結果をstatus・selectionへ反映する。]
    //   副作用: [documentがあればrow構造を変更する。]
    // }
    fn delete_row(&mut self) {
        let result = self
            .document
            .as_mut()
            .map(|document| document.delete_rows(self.selection.row(), 1));
        self.apply_structure_result(result, "Deleted row");
    }

    // {
    //   責務: [insert_column: 選択columnの位置へ1列を挿入する。]
    //   処理: [CsvDocument::insert_columnsを呼び、共通結果処理へ渡す。]
    //   引数: [self: 選択位置・documentを保持する画面状態。]
    //   戻り値: [(): 挿入結果をstatus・selectionへ反映する。]
    //   副作用: [documentがあればcolumn構造を変更する。]
    // }
    fn insert_column(&mut self) {
        let result = self
            .document
            .as_mut()
            .map(|document| document.insert_columns(self.selection.column(), 1));
        self.apply_structure_result(result, "Inserted column");
    }

    // {
    //   責務: [delete_column: 選択columnを1列削除する。]
    //   処理: [CsvDocument::delete_columnsを呼び、共通結果処理へ渡す。]
    //   引数: [self: 選択位置・documentを保持する画面状態。]
    //   戻り値: [(): 削除結果をstatus・selectionへ反映する。]
    //   副作用: [documentがあればcolumn構造を変更する。]
    // }
    fn delete_column(&mut self) {
        let result = self
            .document
            .as_mut()
            .map(|document| document.delete_columns(self.selection.column(), 1));
        self.apply_structure_result(result, "Deleted column");
    }

    // {
    //   責務: [apply_structure_result: row・column構造操作のoption付き結果を画面状態へ反映する。]
    //   処理: [成功時にstatus設定とselection clampを行い、errorまたはdocument不在をstatusへ表示する。]
    //   引数: [self: status・selection・editing state。 result: document不在または構造操作結果。 success: 成功時に使うstatus text。]
    //   戻り値: [(): 操作結果を画面状態へ反映する。]
    // }
    fn apply_structure_result(
        &mut self,
        result: Option<Result<(), crate::process::DocumentError>>,
        success: &str,
    ) {
        match result {
            Some(Ok(())) => {
                self.status = success.to_owned();
                self.clamp_selection();
            }
            Some(Err(error)) => self.status = error.to_string(),
            None => self.status = "CSV ファイルを開いてください".to_owned(),
        }
        self.editing = false;
    }

    // {
    //   責務: [clamp_selection: documentのrow・column範囲外にあるselectionを末尾へ収める。]
    //   処理: [現在位置を各dimensionの最大indexへ制限し、通常のselection更新経路を使う。]
    //   引数: [self: documentとselectionを保持する画面状態。]
    //   戻り値: [(): documentがある場合にselectionを範囲内へ更新する。]
    // }
    fn clamp_selection(&mut self) {
        let Some(document) = self.document.as_ref() else {
            return;
        };
        let row = self
            .selection
            .row()
            .min(document.row_count().saturating_sub(1));
        let column = self
            .selection
            .column()
            .min(document.column_count().saturating_sub(1));
        self.set_selection(CellRef::new(row, column), false);
    }

    // {
    //   責務: [undo: documentの直近commandを取り消し、selectionを有効範囲へ保つ。]
    //   処理: [CsvDocument::undo結果をstatusへ反映し、変更時はselectionをclampする。]
    //   引数: [self: document・selection・statusを保持する画面状態。]
    //   戻り値: [(): undo結果をstatus・selection・editing stateへ反映する。]
    // }
    fn undo(&mut self) {
        match self.document.as_mut().map(CsvDocument::undo) {
            Some(Ok(true)) => {
                self.status = "Undone".to_owned();
                self.clamp_selection();
            }
            Some(Ok(false)) => self.status = "Nothing to undo".to_owned(),
            Some(Err(error)) => self.status = error.to_string(),
            None => {}
        }
        self.editing = false;
    }

    // {
    //   責務: [redo: documentの直近undo commandを再適用し、selectionを有効範囲へ保つ。]
    //   処理: [CsvDocument::redo結果をstatusへ反映し、変更時はselectionをclampする。]
    //   引数: [self: document・selection・statusを保持する画面状態。]
    //   戻り値: [(): redo結果をstatus・selection・editing stateへ反映する。]
    // }
    fn redo(&mut self) {
        match self.document.as_mut().map(CsvDocument::redo) {
            Some(Ok(true)) => {
                self.status = "Redone".to_owned();
                self.clamp_selection();
            }
            Some(Ok(false)) => self.status = "Nothing to redo".to_owned(),
            Some(Err(error)) => self.status = error.to_string(),
            None => {}
        }
        self.editing = false;
    }

    // {
    //   責務: [show_table_preview: documentの全rowをread-only gridとして表示する。]
    //   処理: [row numberとcell textを描画し、header指定時は先頭rowを強調する。]
    //   引数: [self: 未使用の画面状態。 ui: preview描画先。 document: 表示対象。 header: 先頭rowを見出し扱いするか。]
    //   戻り値: [(): document内容を閲覧表示する。]
    // }
    fn show_table_preview(&self, ui: &mut egui::Ui, document: &CsvDocument, header: bool) {
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("csv_preview")
                .striped(true)
                .min_col_width(80.0)
                .show(ui, |ui| {
                    for (row_index, row) in document.rows().enumerate() {
                        ui.weak((row_index + 1).to_string());
                        for value in row {
                            if header && row_index == 0 {
                                ui.strong(value);
                            } else {
                                ui.label(value);
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    }

    // {
    //   責務: [show_status: status・table dimensions・dirty state・zoom controlを下部panelへ表示する。]
    //   処理: [documentの有無とbuffer/document dirty stateを確認し、help以外でzoom sliderを描画する。]
    //   引数: [self: status・document・mode・zoomを保持する画面状態。 ctx: status panel描画先。]
    //   戻り値: [(): status panelを描画し、zoom入力を更新する。]
    // }
    fn show_status(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                if let Some(document) = self.document.as_ref() {
                    ui.separator();
                    ui.label(format!(
                        "{} rows × {} columns",
                        document.row_count(),
                        document.column_count()
                    ));
                    ui.label(if self.code_buffer_dirty {
                        "Code buffer modified"
                    } else if document.is_dirty() {
                        "Modified"
                    } else {
                        "Saved"
                    });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.mode != WorkspaceMode::Help {
                        ui.add(egui::Slider::new(&mut self.zoom, 50.0..=200.0).suffix("%"));
                    }
                });
            });
        });
    }

    // {
    //   責務: [poll_external_changes: watcher通知を受けて外部CSV変更を検出しdocumentを同期する。]
    //   処理: [change hint時にrefresh APIを呼び、競合・reload・watcher errorをstatusとselectionへ反映する。]
    //   引数: [self: watcher・document・editor state・statusを保持する画面状態。]
    //   戻り値: [(): 外部変更がない場合は何もせず、検出時に同期結果を画面状態へ反映する。]
    //   副作用: [外部変更をdocumentへ取り込み、必要に応じてlocal conflict draftを保持する。]
    // }
    fn poll_external_changes(&mut self) {
        let hint = self
            .file_watcher
            .as_mut()
            .map(CsvFileWatcher::take_change_hint);
        let watcher_error = match hint {
            Some(Ok(false)) | None => return,
            Some(Ok(true)) => None,
            Some(Err(error)) => {
                self.file_watcher = None;
                Some(error.to_string())
            }
        };

        let refresh = self
            .document
            .as_mut()
            .map(CsvDocument::refresh_if_external_change);
        let reloaded = matches!(&refresh, Some(Ok(true)));
        let conflict_count = self
            .document
            .as_ref()
            .map_or(0, |document| document.external_conflict_drafts().len());
        self.status = match refresh {
            Some(Ok(true)) => self.document.as_ref().map_or_else(
                || "外部変更を検知しました".to_owned(),
                |document| {
                    if conflict_count == 0 {
                        format!("{} の外部変更を同期しました", document.path().display())
                    } else {
                        format!(
                            "{} の外部変更を同期しました（未解決のローカル変更を{conflict_count}件保持）",
                            document.path().display()
                        )
                    }
                },
            ),
            Some(Ok(false)) => watcher_error.as_ref().map_or_else(
                || self.status.clone(),
                |error| format!("ファイル監視を停止しました: {error}。保存時の確認は有効です"),
            ),
            Some(Err(error)) => error.to_string(),
            None => return,
        };
        if let Some(error) = watcher_error {
            self.status.push_str(&format!(
                "（ファイル監視を停止しました: {error}。保存時の確認は有効です）"
            ));
        }
        if reloaded {
            self.selection = CellRef::new(0, 0);
            self.selection_anchor = self.selection;
            self.reference_input = self.selection.to_string();
            self.editing = false;
            if !self.code_buffer_dirty {
                self.refresh_code_buffer();
            }
            if let Some(document) = self.document.as_ref() {
                let report = document.validation_report();
                if !report.is_valid() {
                    self.status = format!(
                        "{}。{}",
                        self.status,
                        format_validation_violations(report.violations())
                    );
                }
            }
        }
    }
}

impl eframe::App for RowlyApp {
    // {
    //   責務: [update: egui frameごとに外部変更とGUI操作を処理し各panelを描画する。]
    //   処理: [change polling・grid keyboard・menu・toolbar・conflict・status・workspaceの順に実行する。]
    //   引数: [self: frame間で保持するRowlyApp状態。 ctx: 現在のegui context。 _frame: eframe frame handle。]
    //   戻り値: [(): 1 frame分のGUIを更新する。]
    // }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_external_changes();
        self.handle_grid_keys(ctx);
        self.show_menu(ctx);
        self.show_toolbar(ctx);
        self.show_external_conflicts(ctx);
        self.show_status(ctx);
        self.show_workspace(ctx);
    }
}

// {
//   責務: [column_label: 0-based column indexをspreadsheet形式のcolumn labelへ変換する。]
//   処理: [indexを26進相当で分解し、A始まりの大文字列に組み立てる。]
//   引数: [column: 0-based column index。]
//   戻り値: [String: A・Z・AA形式のcolumn label。]
// }
fn column_label(mut column: usize) -> String {
    column += 1;
    let mut letters = Vec::new();
    while column > 0 {
        let remainder = (column - 1) % 26;
        letters.push((b'A' + remainder as u8) as char);
        column = (column - 1) / 26;
    }
    letters.iter().rev().collect()
}

// {
//   責務: [format_validation_violations: 入力規則違反をstatus表示用の短い文字列へ整形する。]
//   処理: [先頭3件のrow・column・value・ruleを列挙し、残件数を付ける。]
//   引数: [violations: documentが返した入力規則違反一覧。]
//   戻り値: [String: 最大3件の詳細と残件数を含むmessage。]
// }
fn format_validation_violations(violations: &[ValidationViolation]) -> String {
    let details = violations
        .iter()
        .take(3)
        .map(|violation| {
            format!(
                "record {}, {} value {:?} rejected by {:?}",
                violation.cell().row(),
                column_label(violation.cell().column()),
                violation.value(),
                violation.rule()
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    let remaining = violations.len().saturating_sub(3);
    if remaining == 0 {
        format!("入力規則違反のため適用できません: {details}")
    } else {
        format!("入力規則違反のため適用できません: {details}; and {remaining} more")
    }
}

// {
//   責務: [cell_range: 2つのcell referenceから包含範囲のrow・column境界を求める。]
//   処理: [各dimensionの小さい方と大きい方を選ぶ。]
//   引数: [first: 範囲端のcell。 second: もう一方の範囲端。]
//   戻り値: [(usize, usize, usize, usize): 最小row・最大row・最小column・最大column。]
// }
fn cell_range(first: CellRef, second: CellRef) -> (usize, usize, usize, usize) {
    (
        first.row().min(second.row()),
        first.row().max(second.row()),
        first.column().min(second.column()),
        first.column().max(second.column()),
    )
}

// {
//   責務: [update_selection: grid selection・anchor・address input・editing stateを同期する。]
//   処理: [referenceをbounds内へclampし、extendでanchor維持を選び、selectionと入力欄を更新する。]
//   引数: [selection: 選択中cell。 anchor: 範囲選択起点。 reference_input: address text。 editing: inline edit状態。 reference: 移動先。 extend: 範囲選択を継続するか。 bounds: row・column count。]
//   戻り値: [(): 4つの画面stateを選択先に同期する。]
// }
fn update_selection(
    selection: &mut CellRef,
    anchor: &mut CellRef,
    reference_input: &mut String,
    editing: &mut bool,
    reference: CellRef,
    extend: bool,
    bounds: (usize, usize),
) {
    let reference = CellRef::new(
        reference.row().min(bounds.0.saturating_sub(1)),
        reference.column().min(bounds.1.saturating_sub(1)),
    );
    if !extend {
        *anchor = reference;
    }
    *selection = reference;
    *reference_input = reference.to_string();
    *editing = false;
}

impl RowlyApp {
    // {
    //   責務: [handle_grid_keys: grid操作中のkeyboard shortcutをselection・edit・undo/redoへ適用する。]
    //   処理: [text inputへfocus中・grid外・inline edit中を除外し、navigationとcommand keyをdispatchする。]
    //   引数: [self: mode・selection・document・editing状態。 ctx: key inputとfocus状態を読むegui context。]
    //   戻り値: [(): keyboard inputに応じた画面操作を実行する。]
    // }
    fn handle_grid_keys(&mut self, ctx: &egui::Context) {
        if self.mode != WorkspaceMode::TableEditor || ctx.wants_keyboard_input() {
            return;
        }
        if self.editing {
            return;
        }

        let (undo, redo, edit, cancel, direction, extend) = ctx.input(|input| {
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
            )
        });

        if undo {
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
