//! ```text
//! 責務: [
//! text_transform: Trim・大文字小文字変換を範囲または列へraw値の明示データ変換として適用する
//! ]
//! 処理: [
//! 1: 対象と変換から変更計画（TextTransformPlan）を作り、件数をpreviewできる
//! 2: 計画をdocumentへ原子的に適用し、1回のUndoで元へ戻せる
//! ]
//! 補足: [
//! 正本のraw文字列だけを変更する。View設定や表示書式は変更しない
//! 変更が無い計画は履歴もdirtyも変えない
//! ]
//! ```
use super::{CellRange, CellRef, CsvDocument, DocumentError};

/// ```text
/// 責務: [
/// TrimMode: 取り除く空白の位置を表す
/// ]
/// フィールド: [
/// Leading: 先頭側の空白
/// Trailing: 末尾側の空白
/// Both: 両側の空白
/// ]
/// 補足: [
/// 空白はUnicodeのWhite_Space属性を持つ文字（Rustのstr::trimと同じ）で、ゼロ幅文字は含まない
/// ]
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrimMode {
    Leading,
    Trailing,
    Both,
}

/// ```text
/// 責務: [
/// CaseMode: 大文字小文字変換の方向を表す
/// ]
/// フィールド: [
/// Upper: 大文字へ変換する
/// Lower: 小文字へ変換する
/// ]
/// 補足: [
/// ロケールに依存しないUnicodeの既定変換（Rustのto_uppercase/to_lowercase）を使う
/// 文字数が変わる変換（ß→SS）や文脈依存（語末のΣ→ς）も既定規則に従う
/// ]
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaseMode {
    Upper,
    Lower,
}

/// ```text
/// 責務: [
/// TextTransform: raw文字列へ適用する変換を表す
/// ]
/// フィールド: [
/// Trim: 空白除去
/// Case: 大文字小文字変換
/// ]
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextTransform {
    Trim(TrimMode),
    Case(CaseMode),
}

/// ```text
/// 責務: [
/// TransformTarget: 変換を適用するcellの範囲を表す
/// ]
/// フィールド: [
/// Range: 矩形範囲。headerを含めて指定した範囲どおりに適用する
/// Column: 1列のデータ行全体。header行は変更しない
/// ]
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransformTarget {
    Range(CellRange),
    Column(usize),
}

/// ```text
/// 責務: [
/// PlannedChange: 1cellの変更前後のraw値を保持する
/// ]
/// フィールド: [
/// reference: 変更するcell
/// before: 計画作成時の値
/// after: 変換後の値
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedChange {
    pub reference: CellRef,
    pub before: String,
    pub after: String,
}

/// ```text
/// 責務: [
/// TextTransformPlan: 変換で変わるcellと走査件数を保持し、適用前に確認できるようにする
/// ]
/// フィールド: [
/// target: 計画作成時の対象
/// transform: 計画作成時の変換
/// scanned: 走査したcell数
/// changes: 値が変わるcellの一覧
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextTransformPlan {
    target: TransformTarget,
    transform: TextTransform,
    scanned: usize,
    changes: Vec<PlannedChange>,
}

impl TextTransformPlan {
    /// ```text
    /// 責務: [
    /// scanned: 走査したcell数を返す
    /// ]
    /// 引数: [
    /// self: 対象の計画
    /// ]
    /// 戻り値: [
    /// usize: 走査したcell数
    /// ]
    /// ```
    pub fn scanned(&self) -> usize {
        self.scanned
    }

    /// ```text
    /// 責務: [
    /// changes: 値が変わるcellの一覧を返す
    /// ]
    /// 引数: [
    /// self: 対象の計画
    /// ]
    /// 戻り値: [
    /// &[PlannedChange]: 変更前後つきの変更一覧
    /// ]
    /// ```
    pub fn changes(&self) -> &[PlannedChange] {
        &self.changes
    }

    /// ```text
    /// 責務: [
    /// change_count: 値が変わるcell数を返す
    /// ]
    /// 引数: [
    /// self: 対象の計画
    /// ]
    /// 戻り値: [
    /// usize: preview用の変更cell数
    /// ]
    /// ```
    pub fn change_count(&self) -> usize {
        self.changes.len()
    }
}

/// ```text
/// 責務: [
/// apply_text_transform: 1cellのraw文字列へ変換を適用する
/// ]
/// 処理: [
/// 1: Trimなら指定位置の空白を除去する
/// 2: Caseなら大文字または小文字へ変換する
/// ]
/// 引数: [
/// transform: 適用する変換
/// value: 変換前のraw値
/// ]
/// 戻り値: [
/// String: 変換後のraw値
/// ]
/// ```
pub fn apply_text_transform(transform: TextTransform, value: &str) -> String {
    match transform {
        TextTransform::Trim(TrimMode::Leading) => value.trim_start().to_owned(),
        TextTransform::Trim(TrimMode::Trailing) => value.trim_end().to_owned(),
        TextTransform::Trim(TrimMode::Both) => value.trim().to_owned(),
        TextTransform::Case(CaseMode::Upper) => value.to_uppercase(),
        TextTransform::Case(CaseMode::Lower) => value.to_lowercase(),
    }
}

impl CsvDocument {
    /// ```text
    /// 責務: [
    /// plan_text_transform: 変換を適用した場合に変わるcellを、documentを変更せずに計算する
    /// ]
    /// 処理: [
    /// 1: 対象を走査するcellの一覧へ展開する（列指定はheader行を除く）
    /// 2: 各cellへ変換を適用し、値が変わるものだけを計画へ入れる
    /// ]
    /// 引数: [
    /// target: 変換対象の範囲または列
    /// transform: 適用する変換
    /// ]
    /// 戻り値: [
    /// TextTransformPlan: 走査件数と変更一覧
    /// ]
    /// エラー: [
    /// DocumentError: 対象のcellまたは列がtable内に存在しない場合
    /// ]
    /// ```
    pub fn plan_text_transform(
        &self,
        target: TransformTarget,
        transform: TextTransform,
    ) -> Result<TextTransformPlan, DocumentError> {
        // 巨大な範囲を展開する前に、範囲がtable内かを確認する
        let (first_row, last_row, first_column, last_column) = match target {
            TransformTarget::Range(range) => {
                let (start, end) = (range.start(), range.end());
                if end.row() >= self.row_count() || end.column() >= self.column_count() {
                    return Err(DocumentError::Edit(format!(
                        "range `{range}` is outside the existing CSV table"
                    )));
                }
                (start.row(), end.row(), start.column(), end.column())
            }
            TransformTarget::Column(column) => {
                if column >= self.column_count() {
                    return Err(DocumentError::Edit(format!(
                        "column {column} is outside the existing CSV table"
                    )));
                }
                // header行は列identityなので、列指定では変更しない
                (1, self.row_count().saturating_sub(1), column, column)
            }
        };

        let mut changes = Vec::new();
        let mut scanned = 0;
        // 列指定でheaderのみの場合は first_row > last_row となり走査しない
        for row in first_row..=last_row {
            for column in first_column..=last_column {
                scanned += 1;
                let reference = CellRef::new(row, column);
                let before = self.cell_ref(reference).ok_or_else(|| {
                    DocumentError::Edit(format!(
                        "cell `{reference}` is outside the existing CSV table"
                    ))
                })?;
                let after = apply_text_transform(transform, before);
                if after != before {
                    changes.push(PlannedChange {
                        reference,
                        before: before.to_owned(),
                        after,
                    });
                }
            }
        }
        Ok(TextTransformPlan {
            target,
            transform,
            scanned,
            changes,
        })
    }

    /// ```text
    /// 責務: [
    /// apply_text_transform_plan: 変換計画をdocumentへ原子的に適用し、1回のUndoにまとめる
    /// ]
    /// 処理: [
    /// 1: 計画作成後にdocumentが変わっていないか、変更前の値で確認する
    /// 2: 全変更を検証規則に照らして確認し、問題が無ければ一括で書き込む
    /// ]
    /// 引数: [
    /// plan: plan_text_transformで作った計画
    /// ]
    /// 戻り値: [
    /// usize: 実際に変更したcell数
    /// ]
    /// 副作用: [
    /// 変更があれば履歴1件とdirty状態が更新される。変更0件なら何も記録しない
    /// ]
    /// エラー: [
    /// DocumentError: 計画作成後にcellが変わった、または変換後の値が検証規則に反する場合（何も変更しない）
    /// ]
    /// ```
    pub fn apply_text_transform_plan(
        &mut self,
        plan: &TextTransformPlan,
    ) -> Result<usize, DocumentError> {
        // 対象全体を再計算し、計画時と同じ変更になるか確認する（変更なしだったcellの後からの変化も検出する）
        let current = self.plan_text_transform(plan.target, plan.transform)?;
        if current.changes != plan.changes {
            return Err(DocumentError::Edit(
                "target cells changed after the transform was planned; plan again".into(),
            ));
        }
        self.set_cell_values(
            plan.changes
                .iter()
                .map(|change| (change.reference, change.after.clone())),
        )?;
        Ok(plan.change_count())
    }

    /// ```text
    /// 責務: [
    /// transform_text: 計画と適用を続けて行い、変更したcell数を返す
    /// ]
    /// 引数: [
    /// target: 変換対象の範囲または列
    /// transform: 適用する変換
    /// ]
    /// 戻り値: [
    /// usize: 変更したcell数
    /// ]
    /// エラー: [
    /// DocumentError: 対象が存在しない、または変換後の値が検証規則に反する場合（何も変更しない）
    /// ]
    /// ```
    pub fn transform_text(
        &mut self,
        target: TransformTarget,
        transform: TextTransform,
    ) -> Result<usize, DocumentError> {
        let plan = self.plan_text_transform(target, transform)?;
        self.apply_text_transform_plan(&plan)
    }
}

#[cfg(test)]
mod tests;
