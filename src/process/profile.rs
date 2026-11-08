//! ```text
//! 責務: [
//! profile: CSV列の件数・空欄・distinct・頻出値・文字数・明示型の統計と入力規則違反数を、値を変更せずに計算する
//! ]
//! 処理: [
//! 1: scope（全canonical rows / 指定rows）の列値をProfileAccumulatorへ1件ずつ渡す
//! 2: 同じscopeの入力規則違反数を既存のvalidation reportから数える
//! 3: 別sourceのaccumulatorをmergeして、multi-sourceの全体統計を作れる
//! ]
//! 補足: [
//! 読み取り専用で、CSV・列型宣言・入力規則を変更しない。型を自動設定しない
//! distinct・頻出値はraw値で数え、00123と123は別の値として数える
//! 明示型がある列だけ最小・最大とparse failureを数える。空欄はparse failureに含めない
//! 異なる値が上限を超えたら、近似せずdistinct・duplicate・頻出値を「計測省略」（None）にする
//! derived（Calculation）結果のprofileは範囲外で、freshな値だけを入力として渡す
//! ]
//! ```
mod accumulator;

pub use accumulator::ProfileAccumulator;

use super::{ColumnType, CsvDocument, DocumentError};

/// ```text
/// 責務: [
/// ProfileScope: profileの対象行を表す
/// ]
/// フィールド: [
/// AllRows: header行を除く全canonical rows
/// Rows: 指定したデータ行（現在Viewの表示行・selectionなど。重複は1回として数える）
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileScope {
    AllRows,
    Rows(Vec<usize>),
}

/// ```text
/// 責務: [
/// ProfileOptions: profile計算の設定を表す
/// ]
/// フィールド: [
/// scope: 対象行
/// column_type: 明示された列型（Noneなら文字列統計だけ。値から推論しない）
/// top_values: 返す頻出値の数
/// distinct_limit: 数える異なる値の上限
/// ]
/// ```
#[derive(Clone, Debug)]
pub struct ProfileOptions {
    pub scope: ProfileScope,
    pub column_type: Option<ColumnType>,
    pub top_values: usize,
    pub distinct_limit: usize,
}

impl Default for ProfileOptions {
    // {
    // 責務: [
    // default: 全行・型なし・頻出10件・distinct上限10万の既定設定を返す
    // ]
    // 引数: []
    // 戻り値: [
    // ProfileOptions: 既定設定
    // ]
    // }
    fn default() -> Self {
        Self {
            scope: ProfileScope::AllRows,
            column_type: None,
            top_values: 10,
            distinct_limit: 100_000,
        }
    }
}

/// ```text
/// 責務: [
/// FrequentValue: 頻出値とその件数を表す
/// ]
/// フィールド: [
/// value: raw値
/// count: 出現数
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrequentValue {
    pub value: String,
    pub count: usize,
}

/// ```text
/// 責務: [
/// NumericRange: 明示数値型で読めた値の最小・最大を元のraw文字列で保持する
/// ]
/// フィールド: [
/// min: 最小値のraw文字列
/// max: 最大値のraw文字列
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumericRange {
    pub min: String,
    pub max: String,
}

/// ```text
/// 責務: [
/// ColumnProfile: 1列のprofile結果を保持する
/// ]
/// フィールド: [
/// column: 対象列（0始まり）
/// rows: 数えたcell数
/// empty: 空文字のcell数
/// non_empty: 空でないcell数
/// distinct: 異なるraw値の数（計測省略ならNone）
/// duplicate: 既出の値と同じcell数（rows - distinct。計測省略ならNone）
/// frequent: 頻出値（件数の多い順、同数はraw値の順）
/// min_length: 最短文字数（Unicode文字単位）
/// max_length: 最長文字数
/// column_type: 使った明示列型
/// parse_failures: 空でないのに明示型として読めない数（型が数値・真偽以外ならNone）
/// numeric_range: 明示数値型の最小・最大
/// boolean_counts: Booleanのtrue数とfalse数
/// validation_violations: 同じscopeの入力規則違反数
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColumnProfile {
    pub column: usize,
    pub rows: usize,
    pub empty: usize,
    pub non_empty: usize,
    pub distinct: Option<usize>,
    pub duplicate: Option<usize>,
    pub frequent: Vec<FrequentValue>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub column_type: Option<ColumnType>,
    pub parse_failures: Option<usize>,
    pub numeric_range: Option<NumericRange>,
    pub boolean_counts: Option<(usize, usize)>,
    pub validation_violations: usize,
}

impl CsvDocument {
    // {
    // 責務: [
    // profile_rows: scopeのデータ行番号を昇順・重複なしで返し、範囲外を拒否する
    // ]
    // 引数: [
    // scope: &ProfileScope
    // ]
    // 戻り値: [
    // Result<Vec<usize>, DocumentError>: 対象のデータ行番号
    // ]
    // }
    fn profile_rows(&self, scope: &ProfileScope) -> Result<Vec<usize>, DocumentError> {
        match scope {
            ProfileScope::AllRows => Ok((1..self.row_count()).collect()),
            ProfileScope::Rows(rows) => {
                let mut rows = rows.clone();
                rows.sort_unstable();
                rows.dedup();
                if let Some(row) = rows
                    .iter()
                    .find(|row| **row == 0 || **row >= self.row_count())
                {
                    return Err(DocumentError::Edit(format!(
                        "row {row} is not a data row of the CSV table"
                    )));
                }
                Ok(rows)
            }
        }
    }

    /// ```text
    /// 責務: [
    /// profile_accumulator: 列のscope内の値を積み上げたaccumulatorを作る（multi-sourceのmerge用）
    /// ]
    /// 引数: [
    /// column: 対象列（0始まり）
    /// options: profile設定
    /// ]
    /// 戻り値: [
    /// ProfileAccumulator: 積み上げ済みの統計
    /// ]
    /// エラー: [
    /// DocumentError: 列がtable外、またはscopeにデータ行以外を含む場合
    /// ]
    /// ```
    pub fn profile_accumulator(
        &self,
        column: usize,
        options: &ProfileOptions,
    ) -> Result<ProfileAccumulator, DocumentError> {
        if column >= self.column_count() {
            return Err(DocumentError::Edit(format!(
                "column {column} is outside the existing CSV table"
            )));
        }
        let mut accumulator = ProfileAccumulator::new(options.column_type, options.distinct_limit);
        for row in self.profile_rows(&options.scope)? {
            if let Some(value) = self.cell(row, column) {
                accumulator.add(value);
            }
        }
        Ok(accumulator)
    }

    /// ```text
    /// 責務: [
    /// profile_column: 列のprofileを、documentを変更せずに計算する
    /// ]
    /// 処理: [
    /// 1: scope内の列値を1件ずつ積み上げる（全値のcloneはしない）
    /// 2: 同じscope・列の入力規則違反数を既存のvalidation reportから数える
    /// ]
    /// 引数: [
    /// column: 対象列（0始まり）
    /// options: profile設定
    /// ]
    /// 戻り値: [
    /// ColumnProfile: 統計
    /// ]
    /// エラー: [
    /// DocumentError: 列がtable外、またはscopeにデータ行以外を含む場合
    /// ]
    /// ```
    pub fn profile_column(
        &self,
        column: usize,
        options: &ProfileOptions,
    ) -> Result<ColumnProfile, DocumentError> {
        let accumulator = self.profile_accumulator(column, options)?;
        let rows = self.profile_rows(&options.scope)?;
        let violations = self
            .validation_report()
            .violations()
            .iter()
            .filter(|violation| {
                let cell = violation.cell();
                cell.column() == column && rows.binary_search(&cell.row()).is_ok()
            })
            .count();
        Ok(accumulator.finish(column, options.top_values, violations))
    }
}

#[cfg(test)]
mod tests;
