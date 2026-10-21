use eframe::egui;

use super::{CellRef, CsvDocument, CsvFileWatcher, RowlyApp, format_validation_violations};

impl RowlyApp {
    // {
    //   責務: [show_external_conflicts: 外部変更とのcell・row構造競合を表示し、選択されたlocal draftを再適用する。]
    //   処理: [競合baseline/local/disk情報を描画し、明示操作時のみCsvDocumentの再適用APIを呼ぶ。]
    //   引数: [self: 競合draftを保持するdocumentとstatus。 ctx: 競合表示領域。]
    //   戻り値: [(): 競合内容を表示し、再適用結果をstatusへ反映する。]
    //   副作用: [選択操作に応じてdocumentのlocal競合値または表全体を再適用する。]
    // }
    pub(super) fn show_external_conflicts(&mut self, ctx: &egui::Context) {
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
    //   責務: [poll_external_changes: watcher通知を受けて外部CSV変更を検出しdocumentを同期する。]
    //   処理: [change hint時にrefresh APIを呼び、競合・reload・watcher errorをstatusとselectionへ反映する。]
    //   引数: [self: watcher・document・editor state・statusを保持する画面状態。]
    //   戻り値: [(): 外部変更がない場合は何もせず、検出時に同期結果を画面状態へ反映する。]
    //   副作用: [外部変更をdocumentへ取り込み、必要に応じてlocal conflict draftを保持する。]
    // }
    pub(super) fn poll_external_changes(&mut self) {
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
                if let Err(error) = self.calculation_engine.recalculate_all(document) {
                    self.status
                        .push_str(&format!("（計算結果を更新できませんでした: {error}）"));
                }
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
