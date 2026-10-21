use crate::{
    process::{
        CellRef, CsvDocument, CsvFileWatcher, ValidationViolation,
        calculation::{CalculationEngine, CalculationStatus, CalculationTrigger},
    },
    project_session::ProjectSession,
};
use eframe::egui;

use crate::gui_commands::{
    CommandRegistry, CommandRegistryError, CoreCommand, core_command_definitions,
};
use crate::process::{CellRef, CsvDocument, CsvFileWatcher, DocumentError, ValidationViolation};

#[path = "gui/command_dispatch.rs"]
mod command_dispatch;


#[path = "gui/calculation_panel.rs"]
mod calculation_panel;
#[path = "gui/clipboard.rs"]
mod clipboard;
#[path = "gui/command_bar.rs"]
mod command_bar;
#[path = "gui/document_actions.rs"]
mod document_actions;
#[path = "gui/document_editor.rs"]
mod document_editor;
#[path = "gui/external_changes.rs"]
mod external_changes;
#[path = "gui_help.rs"]
mod help;
#[path = "gui/table_editor.rs"]
mod table_editor;
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
//   責務: [CalculationCommand: GUIから実行できるCalculation操作を識別する。]
//   選択肢: [Bind: binding登録。 Unbind: binding解除。 Recalculate: 再計算。 Show: editor表示。]
// }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalculationCommand {
    Bind,
    Unbind,
    Recalculate,
    Show,
}

impl CalculationCommand {
    // {
    //   責務: [id: CalculationCommandを外部利用向けのstable command IDへ変換する。]
    //   処理: [variantごとに固定したID文字列を返す。]
    //   引数: [self: IDへ変換するCalculationCommand。]
    //   戻り値: [&'static str: 登録済みのstable command ID。]
    // }
    pub const fn id(self) -> &'static str {
        match self {
            Self::Bind => "calculation.bind",
            Self::Unbind => "calculation.unbind",
            Self::Recalculate => "calculation.recalculate",
            Self::Show => "calculation.show",
        }
    }

    // {
    //   責務: [from_id: 登録済みcommand IDを対応するCalculationCommandへ変換する。]
    //   処理: [登録一覧を検索し、一致するIDのcommandを返す。]
    //   引数: [command_id: 検索するstable command ID。]
    //   戻り値: [Option<Self>: 一致時はcommand、不一致時はNone。]
    // }
    pub fn from_id(command_id: &str) -> Option<Self> {
        CALCULATION_COMMANDS
            .into_iter()
            .find(|command| command.id() == command_id)
    }
}

// {
//   責務: [CALCULATION_COMMANDS: stable IDを持つCalculation commandを登録する。]
//   内容: [Bind: calculation.bind。 Unbind: calculation.unbind。 Recalculate: calculation.recalculate。 Show: calculation.show。]
// }
pub const CALCULATION_COMMANDS: [CalculationCommand; 4] = [
    CalculationCommand::Bind,
    CalculationCommand::Unbind,
    CalculationCommand::Recalculate,
    CalculationCommand::Show,
];

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
//   フィールド: [mode/help_return_mode: 現在画面とhelpからの復帰先。 path_input/reference_input: pathとcell address入力。 code_buffer/code_buffer_dirty: CSV text編集値と未適用状態。 document/file_watcher: 開いたCSVと外部変更監視。 status/zoom: 表示messageと拡大率。 selection/selection_anchor: 選択cellと範囲起点。 editing/edit_value: inline edit状態と入力text。 validation_candidates_open/validation_candidate_index: 入力候補popup状態。 help: help viewer, command_registry: 共通操作定義または登録失敗。]
// }
pub struct RowlyApp {
    mode: WorkspaceMode,
    help_return_mode: WorkspaceMode,
    path_input: String,
    reference_input: String,
    code_buffer: String,
    code_buffer_dirty: bool,
    document: Option<CsvDocument>,
    calculation_engine: CalculationEngine,
    project_session: Option<ProjectSession>,
    project_source_id: Option<String>,
    calculation_bindings_dirty: bool,
    calculation_expression: String,
    calculation_trigger: CalculationTrigger,
    calculation_editor_target: Option<CellRef>,
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
    command_registry: Result<CommandRegistry<CoreCommand>, CommandRegistryError>,
}

impl Default for RowlyApp {
    // {
    //   責務: [default: 初期画面に必要なGUI状態を作る。]
    //   処理: [table editor、空入力、A1選択、既定zoom、空documentとhelp viewerを設定する。]
    //   引数: []
    //   戻り値: [RowlyApp: CSV未読込の初期GUI状態。]
    // }
    fn default() -> Self {
        // 登録不整合がある場合は操作を開始せず、起動時のstatusに理由を残す。
        let command_registry = CommandRegistry::new(core_command_definitions());
        let status = command_registry.as_ref().map_or_else(
            |error| format!("GUI操作の登録に失敗しました: {error}"),
            |_| "CSV ファイルを開いてください".to_owned(),
        );
        Self {
            mode: WorkspaceMode::TableEditor,
            help_return_mode: WorkspaceMode::TableEditor,
            path_input: String::new(),
            reference_input: "A1".to_owned(),
            code_buffer: String::new(),
            code_buffer_dirty: false,
            document: None,
            calculation_engine: CalculationEngine::default(),
            project_session: None,
            project_source_id: None,
            calculation_bindings_dirty: false,
            calculation_expression: String::new(),
            calculation_trigger: CalculationTrigger::DependencyChange,
            calculation_editor_target: None,
            file_watcher: None,
            status,
            zoom: 100.0,
            selection: CellRef::new(0, 0),
            selection_anchor: CellRef::new(0, 0),
            editing: false,
            edit_value: String::new(),
            validation_candidates_open: false,
            validation_candidate_index: 0,
            help: help::HelpViewer::default(),
            command_registry,
        }
    }
}

impl RowlyApp {
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
                    self.show_calculation_panel(ui);
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
                    self.show_table_preview(ui, document, &self.calculation_engine, true);
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
    //   責務: [show_table_preview: documentの全rowをread-only gridとして表示する。]
    //   処理: [row numberとcell valueを描画し、binding targetはderived resultを表示する。]
    //   引数: [self: GUI状態。 ui: preview描画先。 document: canonical CSV。 calculation_engine: derived result source。 header: 先頭rowを見出し扱いするか。]
    //   戻り値: [(): document内容を閲覧表示する。]
    // }
    fn show_table_preview(
        &self,
        ui: &mut egui::Ui,
        document: &CsvDocument,
        calculation_engine: &CalculationEngine,
        header: bool,
    ) {
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("csv_preview")
                .striped(true)
                .min_col_width(80.0)
                .show(ui, |ui| {
                    for (row_index, row) in document.rows().enumerate() {
                        ui.weak((row_index + 1).to_string());
                        for (column_index, value) in row.iter().enumerate() {
                            let derived_result = calculation_engine
                                .bindings()
                                .find(|binding| {
                                    binding.target().single_cell()
                                        == Some(CellRef::new(row_index, column_index))
                                })
                                .and_then(|binding| calculation_engine.result(binding.id()));
                            let display_value = derived_result
                                .and_then(|result| result.value.as_deref())
                                .unwrap_or(value);
                            if header && row_index == 0 {
                                ui.strong(display_value);
                            } else if let Some(result) = derived_result {
                                match &result.status {
                                    CalculationStatus::Evaluated => {
                                        ui.colored_label(egui::Color32::LIGHT_BLUE, display_value);
                                    }
                                    CalculationStatus::Stale => {
                                        ui.colored_label(
                                            egui::Color32::YELLOW,
                                            format!("{display_value} (stale)"),
                                        );
                                    }
                                    CalculationStatus::Error(error) => {
                                        ui.colored_label(
                                            egui::Color32::LIGHT_RED,
                                            format!("#ERROR: {error}"),
                                        );
                                    }
                                    CalculationStatus::Pending => {
                                        ui.weak("(pending)");
                                    }
                                }
                            } else {
                                ui.label(display_value);
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
}

impl eframe::App for RowlyApp {
    // {
    //   責務: [update: egui frameごとに外部変更とGUI操作を処理し各panelを描画する。]
    //   処理: [外部変更、Registry shortcut、grid navigation、各panelを順に更新する。]
    //   引数: [self: frame間で保持するRowlyApp状態。 ctx: 現在のegui context。 _frame: eframe frame handle。]
    //   戻り値: [(): 1 frame分のGUIを更新する。]
    // }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_external_changes();
        self.dispatch_core_shortcuts(ctx);
        self.handle_grid_keys(ctx);
        self.handle_calculation_shortcuts(ctx);
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

impl RowlyApp {}
