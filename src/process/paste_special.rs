//! ```text
//! 責務: [
//! paste_special: clipboardのraw値をTranspose・Repeatで貼り付けるPaste Specialを、計画→原子的適用の明示編集として提供する
//! ]
//! 処理: [
//! 1: clipboardの値（ClipboardGrid）と貼り付け方式から、書込み先範囲と各cellの変更前後を持つ計画（FillPlan）を作る
//! 2: 計画はFillと同じapply_fill_planで適用し、全件の検証後に1回のUndoで書き込む
//! ]
//! 補足: [
//! 貼り付けるのはraw文字列だけで、Calculation BindingやView styleは複製しない
//! 既存table範囲の外へは貼り付けず、行・列を自動で追加しない
//! 行ごとに列数が違うclipboard（ragged）は、既定では拒否し、明示した場合だけ空文字で右を埋める
//! Repeatは選択範囲がclipboardの大きさの整数倍のときだけ許可し、端数のtileを黙って切り捨てない
//! ]
//! ```
use super::clipboard::parse_clipboard_tsv;
use super::{CellRange, CellRef, CsvDocument, DocumentError, FillPlan};

/// ```text
/// 責務: [
/// RaggedPolicy: 行ごとに列数が違うclipboardの扱いを表す
/// ]
/// フィールド: [
/// Reject: 拒否する（既定の安全側）
/// PadEmpty: 短い行の右を空文字で埋めて長方形にする
/// ]
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RaggedPolicy {
    Reject,
    PadEmpty,
}

/// ```text
/// 責務: [
/// PasteMode: Paste Specialの貼り付け方式を表す
/// ]
/// フィールド: [
/// Values: 値をそのまま、選択範囲の左上から貼る
/// Transpose: 行と列を入れ替えて、選択範囲の左上から貼る
/// Repeat: 選択範囲全体へ、値をタイル状に繰り返して貼る
/// ]
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasteMode {
    Values,
    Transpose,
    Repeat,
}

/// ```text
/// 責務: [
/// ClipboardGrid: 貼り付ける長方形のraw値を保持する
/// ]
/// フィールド: [
/// rows: 行ごとのcell値（全行が同じ列数で、1行1列以上）
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardGrid {
    rows: Vec<Vec<String>>,
}

impl ClipboardGrid {
    /// ```text
    /// 責務: [
    /// new: 行の配列から長方形のclipboard値を作る
    /// ]
    /// 引数: [
    /// rows: 行ごとのcell値
    /// ragged: 列数が違う行の扱い
    /// ]
    /// 戻り値: [
    /// ClipboardGrid: 全行が同じ列数に揃った値
    /// ]
    /// エラー: [
    /// DocumentError: 値が空、または列数が違う行がありragged がRejectの場合
    /// ]
    /// ```
    pub fn new(mut rows: Vec<Vec<String>>, ragged: RaggedPolicy) -> Result<Self, DocumentError> {
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        if width == 0 {
            return Err(DocumentError::Edit("the clipboard has no cells".into()));
        }
        for (index, row) in rows.iter_mut().enumerate() {
            if row.len() != width {
                if ragged == RaggedPolicy::Reject {
                    return Err(DocumentError::Edit(format!(
                        "clipboard row {index} has {} cells but the widest row has {width}",
                        row.len()
                    )));
                }
                row.resize(width, String::new());
            }
        }
        Ok(Self { rows })
    }

    /// ```text
    /// 責務: [
    /// from_tsv: clipboardのTSV文字列から長方形のraw値を作る
    /// ]
    /// 引数: [
    /// text: TSV文字列
    /// ragged: 列数が違う行の扱い
    /// ]
    /// 戻り値: [
    /// ClipboardGrid: 解析した値
    /// ]
    /// エラー: [
    /// DocumentError: TSVの解析に失敗した、空だった、またはraggedを拒否する場合
    /// ]
    /// ```
    pub fn from_tsv(text: &str, ragged: RaggedPolicy) -> Result<Self, DocumentError> {
        Self::new(parse_clipboard_tsv(text)?, ragged)
    }

    /// ```text
    /// 責務: [
    /// height: 行数を返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// usize: 行数（1以上）
    /// ]
    /// ```
    pub fn height(&self) -> usize {
        self.rows.len()
    }

    /// ```text
    /// 責務: [
    /// width: 列数を返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// usize: 列数（1以上）
    /// ]
    /// ```
    pub fn width(&self) -> usize {
        self.rows[0].len()
    }

    // {
    // 責務: [
    // value: 指定位置の値を返す
    // ]
    // 引数: [
    // row: usize
    // column: usize
    // ]
    // 戻り値: [
    // &str: 値
    // ]
    // }
    fn value(&self, row: usize, column: usize) -> &str {
        &self.rows[row][column]
    }
}

// {
// 責務: [
// rectangle: 左上と大きさから範囲を作る
// ]
// 引数: [
// start: CellRef 左上
// height: usize
// width: usize
// ]
// 戻り値: [
// Result<CellRange, DocumentError>: 範囲。加算がoverflowする場合はエラー
// ]
// }
fn rectangle(start: CellRef, height: usize, width: usize) -> Result<CellRange, DocumentError> {
    let overflow = || DocumentError::Edit("paste range arithmetic overflowed".into());
    let row = start.row().checked_add(height - 1).ok_or_else(overflow)?;
    let column = start.column().checked_add(width - 1).ok_or_else(overflow)?;
    Ok(CellRange::new(start, CellRef::new(row, column)))
}

impl CsvDocument {
    /// ```text
    /// 責務: [
    /// plan_paste_special: Paste Specialの書込み先と各cellの値を、documentを変更せずに計画する
    /// ]
    /// 処理: [
    /// 1: Values/Transposeは選択範囲の左上を起点に、値（またはその転置）の大きさの範囲を書込み先にする
    /// 2: Repeatは選択範囲全体を書込み先にし、値の大きさの整数倍であることを確認して繰り返す
    /// 3: 書込み先が既存table内にあることを確認し、全cellの変更前後を作る
    /// ]
    /// 引数: [
    /// selection: 貼り付け先の選択範囲
    /// grid: 貼り付ける値
    /// mode: 貼り付け方式
    /// ]
    /// 戻り値: [
    /// FillPlan: 貼り付け計画（適用はapply_fill_plan）
    /// ]
    /// エラー: [
    /// DocumentError: 書込み先がtable外、Repeatの選択範囲が値の大きさの整数倍でない場合
    /// ]
    /// ```
    pub fn plan_paste_special(
        &self,
        selection: CellRange,
        grid: &ClipboardGrid,
        mode: PasteMode,
    ) -> Result<FillPlan, DocumentError> {
        let (height, width) = (grid.height(), grid.width());
        let start = selection.start();
        let target = match mode {
            PasteMode::Values => rectangle(start, height, width)?,
            PasteMode::Transpose => rectangle(start, width, height)?,
            PasteMode::Repeat => {
                let end = selection.end();
                let (rows, columns) = (
                    end.row() - start.row() + 1,
                    end.column() - start.column() + 1,
                );
                if rows % height != 0 || columns % width != 0 {
                    return Err(DocumentError::Edit(format!(
                        "the {rows}x{columns} selection is not a multiple of the {height}x{width} clipboard"
                    )));
                }
                selection
            }
        };
        self.require_range_in_table(target)?;
        let cells = self.planned_cells(target, |reference| {
            let (row, column) = (
                reference.row() - start.row(),
                reference.column() - start.column(),
            );
            Ok(match mode {
                PasteMode::Values => grid.value(row, column),
                PasteMode::Transpose => grid.value(column, row),
                PasteMode::Repeat => grid.value(row % height, column % width),
            }
            .to_owned())
        })?;
        Ok(FillPlan {
            target,
            cells,
            inputs: Vec::new(),
        })
    }

    /// ```text
    /// 責務: [
    /// paste_special: 計画と適用を続けて行い、変更したcell数を返す
    /// ]
    /// 引数: [
    /// selection: 貼り付け先の選択範囲
    /// grid: 貼り付ける値
    /// mode: 貼り付け方式
    /// ]
    /// 戻り値: [
    /// usize: 変更したcell数
    /// ]
    /// エラー: [
    /// DocumentError: 計画または適用に失敗した場合（何も変更しない）
    /// ]
    /// ```
    pub fn paste_special(
        &mut self,
        selection: CellRange,
        grid: &ClipboardGrid,
        mode: PasteMode,
    ) -> Result<usize, DocumentError> {
        let plan = self.plan_paste_special(selection, grid, mode)?;
        self.apply_fill_plan(&plan)
    }
}

#[cfg(test)]
mod tests;
