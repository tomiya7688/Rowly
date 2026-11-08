//! ```text
//! 責務: [
//! fill: raw値のFill Down/Rightと、明示指定した整数・小数のSeries生成を、計画→原子的適用の明示編集として提供する
//! ]
//! 処理: [
//! 1: 対象範囲と方向（またはseries仕様）から変更計画（FillPlan）を作り、書込み値をpreviewできる
//! 2: 計画をdocumentへ適用し、全件の検証後に1回のUndoでまとめて書き込む
//! ]
//! 補足: [
//! Fill Valuesは元cellのraw文字列をそのまま複製し、複数の元cellは繰り返しpatternとして扱う。数値系列へ推測しない
//! Seriesは開始値・増分・長さ（対象範囲の長さ）を明示した場合だけ生成する。文字列から日付などを推測しない
//! 小数は浮動小数点を使わず、10進の整数演算で計算する（0.1+0.2も0.3になる）
//! 既存table範囲の外へは書き込まない。Calculation Bindingは複製も変更もしない
//! Series生成はSeriesGeneratorを実装すれば拡張できる（Date series用の境界）
//! ]
//! ```
use super::{CellRange, CellRef, CsvDocument, DocumentError, PlannedChange};

mod series;
pub use series::{DecimalSeries, IntegerSeries, SeriesGenerator};

/// ```text
/// 責務: [
/// FillDirection: Fill/Seriesを広げる方向を表す
/// ]
/// フィールド: [
/// Down: 上から下へ（列ごと）
/// Right: 左から右へ（行ごと）
/// ]
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FillDirection {
    Down,
    Right,
}

/// ```text
/// 責務: [
/// FillPlan: Fill/Seriesで書き込む全cellの変更前後を保持し、適用前に確認できるようにする
/// ]
/// フィールド: [
/// target: 書き込む範囲
/// cells: 範囲内の全cellの変更前後（変更が無いcellも含む）
/// inputs: 値の元になったcell（Fill Valuesの元cell。Seriesでは空）
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FillPlan {
    pub target: CellRange,
    pub cells: Vec<PlannedChange>,
    pub inputs: Vec<(CellRef, String)>,
}

impl FillPlan {
    /// ```text
    /// 責務: [
    /// change_count: 値が実際に変わるcell数を返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// usize: beforeとafterが異なるcell数
    /// ]
    /// ```
    pub fn change_count(&self) -> usize {
        self.cells.iter().filter(|c| c.before != c.after).count()
    }
}

impl CsvDocument {
    // {
    // 責務: [
    // require_range_in_table: 範囲がすべて既存table内にあることを確認する
    // ]
    // 引数: [
    // range: CellRange
    // ]
    // 戻り値: [
    // Result<(), DocumentError>: table外に出る場合はエラー
    // ]
    // }
    pub(super) fn require_range_in_table(&self, range: CellRange) -> Result<(), DocumentError> {
        let end = range.end();
        if end.row() >= self.row_count() || end.column() >= self.column_count() {
            return Err(DocumentError::Edit(format!(
                "range ends at row {} column {}, outside the existing CSV table",
                end.row(),
                end.column()
            )));
        }
        Ok(())
    }

    // {
    // 責務: [
    // planned_cells: 範囲の各cellへ書く値から全cellの変更前後を作る
    // ]
    // 引数: [
    // range: CellRange
    // value_for: 各cellへ書く値を返す関数
    // ]
    // 戻り値: [
    // Result<Vec<PlannedChange>, DocumentError>: 範囲内の全cell
    // ]
    // }
    pub(super) fn planned_cells(
        &self,
        range: CellRange,
        mut value_for: impl FnMut(CellRef) -> Result<String, DocumentError>,
    ) -> Result<Vec<PlannedChange>, DocumentError> {
        let mut cells = Vec::new();
        for reference in range.iter() {
            let before = self.cell_ref(reference).ok_or_else(|| {
                DocumentError::Edit(format!(
                    "cell `{reference}` is outside the existing CSV table"
                ))
            })?;
            cells.push(PlannedChange {
                reference,
                before: before.to_owned(),
                after: value_for(reference)?,
            });
        }
        Ok(cells)
    }

    /// ```text
    /// 責務: [
    /// plan_fill_values: 範囲の先頭にある元cellのraw値を、残りへ繰り返し複製する計画を作る
    /// ]
    /// 処理: [
    /// 1: 範囲がtable内にあり、元cellの個数が範囲の長さ以下であることを確認する
    /// 2: Downなら各列の上端から、Rightなら各行の左端からpattern_lenセルを元にし、後続へ繰り返す
    /// ]
    /// 引数: [
    /// range: 元cellを含む範囲（元cellは範囲の上端または左端）
    /// direction: 広げる方向
    /// pattern_len: 元cellの個数（1以上。複数なら繰り返しpattern）
    /// ]
    /// 戻り値: [
    /// FillPlan: Fill計画（元cellの値は書き換えない）
    /// ]
    /// エラー: [
    /// DocumentError: pattern_lenが0または範囲より長い、範囲がtable外の場合
    /// ]
    /// ```
    pub fn plan_fill_values(
        &self,
        range: CellRange,
        direction: FillDirection,
        pattern_len: usize,
    ) -> Result<FillPlan, DocumentError> {
        self.require_range_in_table(range)?;
        let (start, end) = (range.start(), range.end());
        let length = match direction {
            FillDirection::Down => end.row() - start.row() + 1,
            FillDirection::Right => end.column() - start.column() + 1,
        };
        if pattern_len == 0 || pattern_len > length {
            return Err(DocumentError::Edit(format!(
                "pattern length {pattern_len} must be between 1 and the fill length {length}"
            )));
        }
        // 元cellの位置から、書込み先cellが参照する元cellを引く
        let source_of = |reference: CellRef| {
            let offset = match direction {
                FillDirection::Down => reference.row() - start.row(),
                FillDirection::Right => reference.column() - start.column(),
            } % pattern_len;
            match direction {
                FillDirection::Down => CellRef::new(start.row() + offset, reference.column()),
                FillDirection::Right => CellRef::new(reference.row(), start.column() + offset),
            }
        };
        let mut inputs = Vec::new();
        for reference in range.iter() {
            let is_source = match direction {
                FillDirection::Down => reference.row() - start.row() < pattern_len,
                FillDirection::Right => reference.column() - start.column() < pattern_len,
            };
            if is_source {
                let value = self.cell_ref(reference).ok_or_else(|| {
                    DocumentError::Edit(format!("cell `{reference}` is outside the table"))
                })?;
                inputs.push((reference, value.to_owned()));
            }
        }
        let cells = self.planned_cells(range, |reference| {
            let source = source_of(reference);
            inputs
                .iter()
                .find(|(cell, _)| *cell == source)
                .map(|(_, value)| value.clone())
                .ok_or_else(|| DocumentError::Edit("fill source is missing".into()))
        })?;
        Ok(FillPlan {
            target: range,
            cells,
            inputs,
        })
    }

    /// ```text
    /// 責務: [
    /// plan_series: 1列または1行の範囲へ、明示したSeriesの値を書く計画を作る
    /// ]
    /// 処理: [
    /// 1: 範囲がtable内の1列（Down）または1行（Right）であることを確認する
    /// 2: 範囲の先頭から順にgeneratorのindex番目の値を割り当てる
    /// ]
    /// 引数: [
    /// range: 書込み先の範囲（長さがseriesの個数になる）
    /// direction: Downなら単一列、Rightなら単一行
    /// generator: 値の生成元
    /// ]
    /// 戻り値: [
    /// FillPlan: Series計画
    /// ]
    /// エラー: [
    /// DocumentError: 範囲が単一の列/行でない、table外、または値を生成できない場合
    /// ]
    /// ```
    pub fn plan_series(
        &self,
        range: CellRange,
        direction: FillDirection,
        generator: &dyn SeriesGenerator,
    ) -> Result<FillPlan, DocumentError> {
        self.require_range_in_table(range)?;
        let (start, end) = (range.start(), range.end());
        let single_line = match direction {
            FillDirection::Down => start.column() == end.column(),
            FillDirection::Right => start.row() == end.row(),
        };
        if !single_line {
            return Err(DocumentError::Edit(
                "a series needs a single column (down) or a single row (right)".into(),
            ));
        }
        let cells = self.planned_cells(range, |reference| {
            let index = match direction {
                FillDirection::Down => reference.row() - start.row(),
                FillDirection::Right => reference.column() - start.column(),
            };
            generator.value(index)
        })?;
        Ok(FillPlan {
            target: range,
            cells,
            inputs: Vec::new(),
        })
    }

    /// ```text
    /// 責務: [
    /// apply_fill_plan: Fill/Series計画を1回のUndoで適用する
    /// ]
    /// 処理: [
    /// 1: 元cellと対象cellが計画時から変わっていないことを確認し、変わっていれば拒否する
    /// 2: 値が変わるcellだけをset_cell_valuesで全件検証して書き込む
    /// ]
    /// 引数: [
    /// plan: &FillPlan
    /// ]
    /// 戻り値: [
    /// usize: 変更したcell数
    /// ]
    /// エラー: [
    /// DocumentError: 計画が古い、または検証規則に反する場合（何も変更しない）
    /// ]
    /// ```
    pub fn apply_fill_plan(&mut self, plan: &FillPlan) -> Result<usize, DocumentError> {
        let stale = plan
            .inputs
            .iter()
            .any(|(reference, value)| self.cell_ref(*reference) != Some(value.as_str()))
            || plan
                .cells
                .iter()
                .any(|c| self.cell_ref(c.reference) != Some(c.before.as_str()));
        if stale {
            return Err(DocumentError::Edit(
                "cells changed after the fill was planned; plan again".into(),
            ));
        }
        self.set_cell_values(
            plan.cells
                .iter()
                .filter(|c| c.before != c.after)
                .map(|c| (c.reference, c.after.clone())),
        )?;
        Ok(plan.change_count())
    }

    /// ```text
    /// 責務: [
    /// fill_values: 計画と適用を続けて行い、変更したcell数を返す
    /// ]
    /// 引数: [
    /// range: 元cellを含む範囲
    /// direction: 広げる方向
    /// pattern_len: 元cellの個数
    /// ]
    /// 戻り値: [
    /// usize: 変更したcell数
    /// ]
    /// エラー: [
    /// DocumentError: 計画または適用に失敗した場合（何も変更しない）
    /// ]
    /// ```
    pub fn fill_values(
        &mut self,
        range: CellRange,
        direction: FillDirection,
        pattern_len: usize,
    ) -> Result<usize, DocumentError> {
        let plan = self.plan_fill_values(range, direction, pattern_len)?;
        self.apply_fill_plan(&plan)
    }

    /// ```text
    /// 責務: [
    /// generate_series: 計画と適用を続けて行い、変更したcell数を返す
    /// ]
    /// 引数: [
    /// range: 書込み先の単一列/単一行
    /// direction: 方向
    /// generator: 値の生成元
    /// ]
    /// 戻り値: [
    /// usize: 変更したcell数
    /// ]
    /// エラー: [
    /// DocumentError: 計画または適用に失敗した場合（何も変更しない）
    /// ]
    /// ```
    pub fn generate_series(
        &mut self,
        range: CellRange,
        direction: FillDirection,
        generator: &dyn SeriesGenerator,
    ) -> Result<usize, DocumentError> {
        let plan = self.plan_series(range, direction, generator)?;
        self.apply_fill_plan(&plan)
    }
}

#[cfg(test)]
mod tests;
