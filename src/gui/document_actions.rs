use eframe::egui;
use std::path::PathBuf;

use crate::process::calculation::CalculationEngine;

use crate::project::SourceKind;

use super::{CellRef, CsvDocument, CsvFileWatcher, ProjectSession, RowlyApp};

impl RowlyApp {
    // {
    //   責務: [open_csv: 入力pathのCSVを開き、documentと関連するGUI状態を初期化する。]
    //   処理: [CsvDocumentを開き、text bufferとfile watcherを準備し、selectionとstatusを更新する。]
    //   引数: [self: 更新対象の画面状態。 ctx: file change時にrepaintするegui context。]
    //   戻り値: [(): 成功・失敗をstatusへ反映する。]
    //   エラー: [open・CSV text取得・file watcher開始の失敗をstatusへ表示する。]
    // }
    pub(super) fn open_csv(&mut self, ctx: &egui::Context) {
        let path = PathBuf::from(self.path_input.trim());
        if path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("rwprj") || extension.eq_ignore_ascii_case("rowlyx")
        }) {
            self.open_project(path, ctx);
            return;
        }
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
                self.calculation_engine = CalculationEngine::default();
                self.project_session = None;
                self.project_source_id = None;
                self.calculation_bindings_dirty = false;
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

    fn open_project(&mut self, path: PathBuf, ctx: &egui::Context) {
        let result = (|| {
            let session = ProjectSession::open(&path).map_err(|error| error.to_string())?;
            let source = session
                .project()
                .sources
                .iter()
                .find(|source| source.kind == SourceKind::File)
                .ok_or_else(|| "projectにfile sourceがありません".to_owned())?;
            let source_id = source.id.clone();
            let (document, calculation_engine) = session
                .open_source_with_calculations(&source_id)
                .map_err(|error| error.to_string())?;
            Ok::<_, String>((session, source_id, document, calculation_engine))
        })();
        match result {
            Ok((session, source_id, document, calculation_engine)) => {
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
                self.status = format!("{} のsource `{source_id}` を開きました", path.display());
                self.path_input = path.display().to_string();
                self.code_buffer = code_buffer;
                self.code_buffer_dirty = false;
                self.document = Some(document);
                self.file_watcher = watcher.ok();
                self.calculation_engine = calculation_engine;
                self.project_session = Some(session);
                self.project_source_id = Some(source_id);
                self.calculation_bindings_dirty = false;
                self.selection = CellRef::new(0, 0);
                self.selection_anchor = self.selection;
                self.reference_input = self.selection.to_string();
                self.editing = false;
                self.validation_candidates_open = false;
            }
            Err(error) => self.status = error,
        }
    }

    // {
    //   責務: [save_csv: 開いているdocumentを保存し結果をstatusへ表示する。]
    //   処理: [CsvDocument::saveを呼び、保存先またはerrorをstatusへ設定する。]
    //   引数: [self: 保存対象documentを保持する画面状態。]
    //   戻り値: [(): 保存成否をstatusへ反映する。]
    // }
    pub(super) fn save_csv(&mut self) {
        let Some(document) = self.document.as_mut() else {
            self.status = "保存する CSV がありません".to_owned();
            return;
        };
        if let Err(error) = document.save() {
            self.status = error.to_string();
            return;
        }
        let saved_path = document.path().display().to_string();
        if self.project_session.is_some() && !self.save_project_calculations() {
            return;
        }
        self.status = format!("{saved_path} を保存しました");
    }

    fn save_project_calculations(&mut self) -> bool {
        let (Some(session), Some(source_id), Some(document)) = (
            self.project_session.as_mut(),
            self.project_source_id.as_deref(),
            self.document.as_ref(),
        ) else {
            return false;
        };
        match session.save_generated_calculation_bindings(
            source_id,
            document,
            &self.calculation_engine,
        ) {
            Ok(()) => match session.save() {
                Ok(()) => {
                    self.calculation_bindings_dirty = false;
                    true
                }
                Err(error) => {
                    self.status = error.to_string();
                    false
                }
            },
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    // {
    //   責務: [open_project_source: project内のsourceを切り替え、計算規則とCSVを再読込する。]
    //   処理: [未保存変更を検査し、ProjectSessionからdocumentとCalculationEngineを復元する。]
    //   引数: [source_id: 開くsource ID。 previous_source_id: 失敗時に戻すsource ID。 ctx: watcher通知用context。]
    //   戻り値: [(): 成功時にsourceを置換し、失敗時は現在のsourceを保つ。]
    // }
    pub(super) fn open_project_source(
        &mut self,
        source_id: &str,
        previous_source_id: Option<&str>,
        ctx: &egui::Context,
    ) {
        let has_unsaved_changes = self.calculation_bindings_dirty
            || self.code_buffer_dirty
            || self.document.as_ref().is_some_and(CsvDocument::is_dirty);
        if has_unsaved_changes {
            self.status = "sourceを切り替える前に変更をSaveしてください".to_owned();
            self.project_source_id = previous_source_id.map(str::to_owned);
            return;
        }
        let opened = self
            .project_session
            .as_ref()
            .ok_or_else(|| "project session is not open".to_owned())
            .and_then(|session| {
                session
                    .open_source_with_calculations(source_id)
                    .map_err(|error| error.to_string())
            });
        match opened {
            Ok((document, calculation_engine)) => match document.csv_text() {
                Ok(code_buffer) => {
                    let repaint_context = ctx.clone();
                    let watcher = CsvFileWatcher::new(document.path(), move || {
                        repaint_context.request_repaint();
                    });
                    self.code_buffer = code_buffer;
                    self.code_buffer_dirty = false;
                    self.document = Some(document);
                    self.file_watcher = watcher.ok();
                    self.calculation_engine = calculation_engine;
                    self.calculation_bindings_dirty = false;
                    self.calculation_editor_target = None;
                    self.selection = CellRef::new(0, 0);
                    self.selection_anchor = self.selection;
                    self.reference_input = self.selection.to_string();
                    self.editing = false;
                    self.validation_candidates_open = false;
                    self.status = format!("project source `{source_id}` を開きました");
                }
                Err(error) => {
                    self.status = error.to_string();
                    self.project_source_id = previous_source_id.map(str::to_owned);
                }
            },
            Err(error) => {
                self.status = error;
                self.project_source_id = previous_source_id.map(str::to_owned);
            }
        }
    }
}
