use eframe::egui;
use std::path::PathBuf;

use crate::process::CsvDocument;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum WorkspaceMode {
    #[default]
    TableEditor,
    TextEditor,
    Viewer,
}

pub struct RowlyApp {
    mode: WorkspaceMode,
    path_input: String,
    document: Option<CsvDocument>,
    status: String,
    zoom: f32,
}

impl Default for RowlyApp {
    fn default() -> Self {
        Self {
            mode: WorkspaceMode::TableEditor,
            path_input: String::new(),
            document: None,
            status: "CSV ファイルを開いてください".to_owned(),
            zoom: 100.0,
        }
    }
}

impl RowlyApp {
    fn open_csv(&mut self) {
        let path = PathBuf::from(self.path_input.trim());
        match CsvDocument::open(&path) {
            Ok(document) => {
                self.status = format!("{} を開きました", path.display());
                self.path_input = path.display().to_string();
                self.document = Some(document);
            }
            Err(error) => self.status = error.to_string(),
        }
    }

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

    fn show_menu(&mut self, ctx: &egui::Context) {
        let mut open_requested = false;
        let mut save_requested = false;
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
                    ui.label("Undo / Redo controls will be added with the grid workflow.");
                });
                ui.menu_button("View", |ui| {
                    ui.label("Choose a workspace mode from the toolbar.");
                });
                ui.separator();
                ui.label("Rowly");
            });
        });
        if open_requested {
            self.open_csv();
        }
        if save_requested {
            self.save_csv();
        }
    }

    fn show_toolbar(&mut self, ctx: &egui::Context) {
        let mut open_requested = false;
        let mut save_requested = false;
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
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
                    .add_enabled(self.document.is_some(), egui::Button::new("Save"))
                    .clicked()
                {
                    save_requested = true;
                }
                ui.separator();
                ui.selectable_value(&mut self.mode, WorkspaceMode::TableEditor, "Table Editor");
                ui.selectable_value(&mut self.mode, WorkspaceMode::TextEditor, "Text Editor");
                ui.selectable_value(&mut self.mode, WorkspaceMode::Viewer, "Viewer");
            });
        });
        if open_requested {
            self.open_csv();
        }
        if save_requested {
            self.save_csv();
        }
    }

    fn show_workspace(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(document) = self.document.as_ref() else {
                ui.vertical_centered(|ui| {
                    ui.add_space(120.0);
                    ui.heading("Rowly");
                    ui.label("CSV を開くと、ここにワークスペースが表示されます。");
                });
                return;
            };

            match self.mode {
                WorkspaceMode::TableEditor => {
                    ui.heading("Table Editor");
                    ui.label("表の編集グリッドは後続の Issue #57 で実装します。");
                    self.show_table_preview(ui, document, false);
                }
                WorkspaceMode::TextEditor => {
                    ui.heading("Text Editor");
                    ui.label("テキスト編集領域はこのシェルでは表示用です。");
                    let mut text = document
                        .rows()
                        .map(|row| row.join(","))
                        .collect::<Vec<_>>()
                        .join("\n");
                    egui::ScrollArea::both().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut text)
                                .font(egui::TextStyle::Monospace)
                                .interactive(false)
                                .desired_width(f32::INFINITY)
                                .desired_rows(20),
                        );
                    });
                }
                WorkspaceMode::Viewer => {
                    ui.heading("Viewer");
                    self.show_table_preview(ui, document, true);
                }
            }
        });
    }

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
                    ui.label(if document.is_dirty() {
                        "Modified"
                    } else {
                        "Saved"
                    });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::Slider::new(&mut self.zoom, 50.0..=200.0).suffix("%"));
                });
            });
        });
    }
}

impl eframe::App for RowlyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show_menu(ctx);
        self.show_toolbar(ctx);
        self.show_status(ctx);
        self.show_workspace(ctx);
    }
}
