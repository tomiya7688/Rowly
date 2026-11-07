//! ```text
//! 責務: [
//! column_reshape: 1列を複数列へ分割するSplit Columnと、複数列を1列へ結合するCombine Columnsを、計画→原子的適用の明示データ変換として提供する
//! ]
//! 処理: [
//! 1: 対象列・区切り・新しいheaderから変更計画（SplitPlan/CombinePlan）を作り、新しい列の値をpreviewできる
//! 2: 計画をdocumentへ適用し、列追加・値設定・元列削除を1回のUndoにまとめる
//! ]
//! 補足: [
//! 区切りや結合はCSVのraw文字列だけで行い、表示書式・型解釈は使わない
//! header行（0行目）は分割・結合の対象にせず、新しい列のheaderには指定したheaderを書く
//! 新しいheaderは、残る既存headerおよび新しいheader同士と重複できない（列identityを一意に保つため）
//! 対象列より短い行（欠けたcell）があるtableは、値を推測せず拒否する
//! 列の追加・削除は既存のinsert_columns/delete_columnsを使うため、検証ruleのtarget indexは同じ規則で追従する
//! View設定・Calculation Bindingの再解決は構造変更通知側（#343）の範囲で、ここでは扱わない
//! 1つのCsvDocumentは1つのphysical sourceなので、source間の列は結合しない
//! ]
//! ```
use super::{CellRef, CsvDocument, DocumentError};

/// ```text
/// 責務: [
/// SplitPlan: 列分割で作る新しい列の値と配置を保持し、適用前に確認できるようにする
/// ]
/// フィールド: [
/// column: 分割する元の列
/// delimiter: 区切り文字列（空でない）
/// headers: 新しい列のheader（出力列数と同じ長さ）
/// keep_source: 元の列を残すか
/// scanned: 分割したデータ行数
/// pieces: データ行ごとの分割結果（行番号と、出力列数ぶんの文字列）
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitPlan {
    pub column: usize,
    pub delimiter: String,
    pub headers: Vec<String>,
    pub keep_source: bool,
    pub scanned: usize,
    pub pieces: Vec<(usize, Vec<String>)>,
}

/// ```text
/// 責務: [
/// CombinePlan: 列結合で作る新しい列の値と配置を保持し、適用前に確認できるようにする
/// ]
/// フィールド: [
/// columns: 結合する列（結合順）
/// separator: 結合に使う文字列
/// header: 新しい列のheader
/// keep_sources: 元の列を残すか
/// scanned: 結合したデータ行数
/// values: データ行ごとの結合結果（行番号と文字列）
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CombinePlan {
    pub columns: Vec<usize>,
    pub separator: String,
    pub header: String,
    pub keep_sources: bool,
    pub scanned: usize,
    pub values: Vec<(usize, String)>,
}

impl CsvDocument {
    // {
    // 責務: [
    // require_columns_present: 指定列がすべて全行に存在することを確認する
    // ]
    // 引数: [
    // columns: &[usize]
    // ]
    // 戻り値: [
    // Result<(), DocumentError>: 列が範囲外、または欠けたcellがある場合はエラー
    // ]
    // }
    fn require_columns_present(&self, columns: &[usize]) -> Result<(), DocumentError> {
        if let Some(column) = columns.iter().find(|c| **c >= self.column_count()) {
            return Err(DocumentError::Edit(format!(
                "column {column} is outside the existing CSV table"
            )));
        }
        for (row, cells) in self.rows().enumerate() {
            if let Some(column) = columns.iter().find(|c| **c >= cells.len()) {
                return Err(DocumentError::Edit(format!(
                    "row {row} has no cell in column {column}; fix the ragged row first"
                )));
            }
        }
        Ok(())
    }

    // {
    // 責務: [
    // check_new_headers: 新しいheaderが空でなく、互いにも残る既存headerとも重複しないことを確認する
    // ]
    // 引数: [
    // new_headers: &[String]
    // removed_columns: &[usize] 適用後に消える列（比較から除く）
    // ]
    // 戻り値: [
    // Result<(), DocumentError>: 重複または空headerがある場合はエラー
    // ]
    // }
    fn check_new_headers(
        &self,
        new_headers: &[String],
        removed_columns: &[usize],
    ) -> Result<(), DocumentError> {
        for (index, header) in new_headers.iter().enumerate() {
            if header.is_empty() {
                return Err(DocumentError::Edit("a new header must not be empty".into()));
            }
            if new_headers[..index].contains(header) {
                return Err(DocumentError::Edit(format!(
                    "new header `{header}` is specified more than once"
                )));
            }
            let clashes = (0..self.column_count())
                .filter(|c| !removed_columns.contains(c))
                .any(|c| self.cell(0, c) == Some(header.as_str()));
            if clashes {
                return Err(DocumentError::Edit(format!(
                    "header `{header}` already exists in the table"
                )));
            }
        }
        Ok(())
    }

    /// ```text
    /// 責務: [
    /// plan_split_column: 列を区切りで分割する計画を、documentを変更せずに作る
    /// ]
    /// 処理: [
    /// 1: 区切り・header数・対象列・欠けたcellを検証する
    /// 2: 各データ行の値をdelimiterで最大「出力列数」個に分け、足りない分は空文字にする（最後の列には残りをそのまま入れる）
    /// ]
    /// 引数: [
    /// column: 分割する列
    /// delimiter: 区切り文字列（空は不可）
    /// headers: 新しい列のheader（1個以上。個数が出力列数になる）
    /// keep_source: 元の列を残すか
    /// ]
    /// 戻り値: [
    /// SplitPlan: 分割計画
    /// ]
    /// エラー: [
    /// DocumentError: 区切りが空、headerが無い・重複・空、列が範囲外、または欠けたcellがある場合
    /// ]
    /// ```
    pub fn plan_split_column(
        &self,
        column: usize,
        delimiter: &str,
        headers: &[String],
        keep_source: bool,
    ) -> Result<SplitPlan, DocumentError> {
        if delimiter.is_empty() {
            return Err(DocumentError::Edit(
                "the split delimiter must not be empty".into(),
            ));
        }
        if headers.is_empty() {
            return Err(DocumentError::Edit(
                "at least one new header is required to split a column".into(),
            ));
        }
        self.require_columns_present(&[column])?;
        let removed: &[usize] = if keep_source { &[] } else { &[column] };
        self.check_new_headers(headers, removed)?;

        let parts = headers.len();
        let mut pieces = Vec::new();
        for row in 1..self.row_count() {
            let text = self.cell(row, column).unwrap_or_default();
            let mut values: Vec<String> =
                text.splitn(parts, delimiter).map(str::to_owned).collect();
            values.resize(parts, String::new());
            pieces.push((row, values));
        }
        Ok(SplitPlan {
            column,
            delimiter: delimiter.to_owned(),
            headers: headers.to_vec(),
            keep_source,
            scanned: self.row_count().saturating_sub(1),
            pieces,
        })
    }

    /// ```text
    /// 責務: [
    /// apply_split_plan: 分割計画をdocumentへ1回のUndoで適用する
    /// ]
    /// 処理: [
    /// 1: 現在のdocumentで計画を作り直し、古い計画なら拒否する
    /// 2: 元列の直後へ列を追加し、headerと分割値を設定し、必要なら元列を削除する
    /// ]
    /// 引数: [
    /// plan: &SplitPlan
    /// ]
    /// 戻り値: [
    /// usize: 追加した列数
    /// ]
    /// エラー: [
    /// DocumentError: 計画が古い場合、または適用に失敗した場合（途中の変更はrollbackする）
    /// ]
    /// ```
    pub fn apply_split_plan(&mut self, plan: &SplitPlan) -> Result<usize, DocumentError> {
        let current = self.plan_split_column(
            plan.column,
            &plan.delimiter,
            &plan.headers,
            plan.keep_source,
        )?;
        if &current != plan {
            return Err(DocumentError::Edit(
                "rows changed after the split was planned; plan again".into(),
            ));
        }
        let first = plan.column + 1;
        let mut values = Vec::new();
        for (offset, header) in plan.headers.iter().enumerate() {
            values.push((CellRef::new(0, first + offset), header.clone()));
        }
        for (row, parts) in &plan.pieces {
            for (offset, text) in parts.iter().enumerate() {
                values.push((CellRef::new(*row, first + offset), text.clone()));
            }
        }
        let count = plan.headers.len();
        let deletes: &[usize] = if plan.keep_source {
            &[]
        } else {
            &[plan.column]
        };
        self.apply_reshape(first, count, values, deletes)?;
        Ok(count)
    }

    /// ```text
    /// 責務: [
    /// split_column: 計画と適用を続けて行い、追加した列数を返す
    /// ]
    /// 引数: [
    /// column: 分割する列
    /// delimiter: 区切り文字列
    /// headers: 新しい列のheader
    /// keep_source: 元の列を残すか
    /// ]
    /// 戻り値: [
    /// usize: 追加した列数
    /// ]
    /// エラー: [
    /// DocumentError: 計画または適用に失敗した場合（何も変更しない）
    /// ]
    /// ```
    pub fn split_column(
        &mut self,
        column: usize,
        delimiter: &str,
        headers: &[String],
        keep_source: bool,
    ) -> Result<usize, DocumentError> {
        let plan = self.plan_split_column(column, delimiter, headers, keep_source)?;
        self.apply_split_plan(&plan)
    }

    /// ```text
    /// 責務: [
    /// plan_combine_columns: 複数列をseparatorで結合する計画を、documentを変更せずに作る
    /// ]
    /// 処理: [
    /// 1: 列が2個以上で重複せず、headerが有効で、欠けたcellが無いことを検証する
    /// 2: 各データ行で、指定した列順にraw値をseparatorでつなぐ（空文字も飛ばさない）
    /// ]
    /// 引数: [
    /// columns: 結合する列（結合順）
    /// separator: 結合に使う文字列（空文字可）
    /// header: 新しい列のheader
    /// keep_sources: 元の列を残すか
    /// ]
    /// 戻り値: [
    /// CombinePlan: 結合計画
    /// ]
    /// エラー: [
    /// DocumentError: 列が2個未満・重複・範囲外、headerが空または重複、欠けたcellがある場合
    /// ]
    /// ```
    pub fn plan_combine_columns(
        &self,
        columns: &[usize],
        separator: &str,
        header: &str,
        keep_sources: bool,
    ) -> Result<CombinePlan, DocumentError> {
        if columns.len() < 2 {
            return Err(DocumentError::Edit(
                "at least two columns are required to combine".into(),
            ));
        }
        for (index, column) in columns.iter().enumerate() {
            if columns[..index].contains(column) {
                return Err(DocumentError::Edit(format!(
                    "column {column} is specified more than once"
                )));
            }
        }
        self.require_columns_present(columns)?;
        let removed: &[usize] = if keep_sources { &[] } else { columns };
        self.check_new_headers(&[header.to_owned()], removed)?;

        let mut values = Vec::new();
        for row in 1..self.row_count() {
            let joined = columns
                .iter()
                .map(|c| self.cell(row, *c).unwrap_or_default())
                .collect::<Vec<_>>()
                .join(separator);
            values.push((row, joined));
        }
        Ok(CombinePlan {
            columns: columns.to_vec(),
            separator: separator.to_owned(),
            header: header.to_owned(),
            keep_sources,
            scanned: self.row_count().saturating_sub(1),
            values,
        })
    }

    /// ```text
    /// 責務: [
    /// apply_combine_plan: 結合計画をdocumentへ1回のUndoで適用する
    /// ]
    /// 処理: [
    /// 1: 現在のdocumentで計画を作り直し、古い計画なら拒否する
    /// 2: 最も右の元列の直後へ列を追加し、headerと結合値を設定し、必要なら元列を右から削除する
    /// ]
    /// 引数: [
    /// plan: &CombinePlan
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: 計画が古い場合、または適用に失敗した場合（途中の変更はrollbackする）
    /// ]
    /// ```
    pub fn apply_combine_plan(&mut self, plan: &CombinePlan) -> Result<(), DocumentError> {
        let current = self.plan_combine_columns(
            &plan.columns,
            &plan.separator,
            &plan.header,
            plan.keep_sources,
        )?;
        if &current != plan {
            return Err(DocumentError::Edit(
                "rows changed after the combine was planned; plan again".into(),
            ));
        }
        let rightmost = plan.columns.iter().copied().max().unwrap_or(0);
        let target = rightmost + 1;
        let mut values = vec![(CellRef::new(0, target), plan.header.clone())];
        for (row, text) in &plan.values {
            values.push((CellRef::new(*row, target), text.clone()));
        }
        // 右の列から消すと、左の列の位置がずれない
        let mut deletes: Vec<usize> = if plan.keep_sources {
            Vec::new()
        } else {
            plan.columns.clone()
        };
        deletes.sort_unstable_by(|a, b| b.cmp(a));
        self.apply_reshape(target, 1, values, &deletes)
    }

    /// ```text
    /// 責務: [
    /// combine_columns: 計画と適用を続けて行う
    /// ]
    /// 引数: [
    /// columns: 結合する列（結合順）
    /// separator: 結合に使う文字列
    /// header: 新しい列のheader
    /// keep_sources: 元の列を残すか
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: 計画または適用に失敗した場合（何も変更しない）
    /// ]
    /// ```
    pub fn combine_columns(
        &mut self,
        columns: &[usize],
        separator: &str,
        header: &str,
        keep_sources: bool,
    ) -> Result<(), DocumentError> {
        let plan = self.plan_combine_columns(columns, separator, header, keep_sources)?;
        self.apply_combine_plan(&plan)
    }

    // {
    // 責務: [
    // apply_reshape: 列追加・値設定・複数列削除を1つのtransactionで行い、失敗時はrollbackする
    // ]
    // 引数: [
    // index: usize 追加する位置
    // count: usize 追加する列数
    // values: Vec<(CellRef, String)> 設定する値
    // deletes: &[usize] 並べた順に削除する列
    // ]
    // 戻り値: [
    // Result<(), DocumentError>: 失敗時はrollback済みで、rollbackも失敗した場合は両方を伝える
    // ]
    // }
    fn apply_reshape(
        &mut self,
        index: usize,
        count: usize,
        values: Vec<(CellRef, String)>,
        deletes: &[usize],
    ) -> Result<(), DocumentError> {
        self.begin_transaction()?;
        let result = self.reshape_steps(index, count, values, deletes);
        match result {
            Ok(()) => self.commit_transaction(),
            Err(error) => Err(match self.rollback_transaction() {
                Ok(()) => error,
                Err(rollback) => {
                    DocumentError::Edit(format!("{error}; rollback also failed: {rollback}"))
                }
            }),
        }
    }

    // {
    // 責務: [
    // reshape_steps: transaction内で列追加・値設定・列削除を順に行う
    // ]
    // 引数: [
    // index: usize
    // count: usize
    // values: Vec<(CellRef, String)>
    // deletes: &[usize]
    // ]
    // 戻り値: [
    // Result<(), DocumentError>: 最初に失敗した操作のエラー
    // ]
    // }
    fn reshape_steps(
        &mut self,
        index: usize,
        count: usize,
        values: Vec<(CellRef, String)>,
        deletes: &[usize],
    ) -> Result<(), DocumentError> {
        // Header対象のruleは名前で列を引くため、header再利用で新しい列へ移った違反を適用後に検出する
        let violations_before = self.validation_report().violations.len();
        self.insert_columns(index, count)?;
        self.set_cell_values(values)?;
        for column in deletes {
            self.delete_columns(*column, 1)?;
        }
        if self.validation_report().violations.len() > violations_before {
            return Err(DocumentError::Edit(
                "the reshaped table would violate a validation rule".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
