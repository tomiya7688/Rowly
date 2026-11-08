//! ```text
//! 責務: [
//! sort_rows: View Sortと同じ並び替えkeyを、single sourceのCSV行順へ明示Commandとして反映する
//! ]
//! 処理: [
//! 1: Sort key（列・方向・比較方式）から、全データ行の新しい並び（RowOrderPlan）をdocumentを変えずに作る
//! 2: 計画をapply_row_order_planで適用し、全行を1回のUndoで並べ替える
//! ]
//! 補足: [
//! Filterは行の削除に使わない。Filterで非表示の行も含めて、全データ行を並べ替える
//! header行（0行目）は動かさず、データ行（1行目以降）だけを並べ替える
//! 同順位の行は元の行順を保つstable sortで、実行のたびに順番が揺れない
//! Number比較では数値として読めないcell（空・文字・NaN・無限大）を、方向に関係なく末尾へ安定配置する
//! 複数physical sourceにまたがるlogical tableは、暗黙に行をsource間で移さず拒否する
//! ]
//! ```
use std::cmp::Ordering;

use super::{CsvDocument, DocumentError};

/// ```text
/// 責務: [
/// SortDirection: 並び替えの方向を表す
/// ]
/// フィールド: [
/// Ascending: 昇順
/// Descending: 降順
/// ]
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// ```text
/// 責務: [
/// SortKind: cell値の比較方式を表す
/// ]
/// フィールド: [
/// Raw: raw文字列をバイト順で比較する（暗黙の型推論をしない）
/// Number: 数値として明示的に比較する。読めないcellは末尾へまとめる
/// ]
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortKind {
    Raw,
    Number,
}

/// ```text
/// 責務: [
/// SortKey: 並び替えに使う1つのkeyを表す
/// ]
/// フィールド: [
/// column: 比較する列（0始まり）
/// direction: 昇順か降順か
/// kind: 比較方式
/// ]
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SortKey {
    pub column: usize,
    pub direction: SortDirection,
    pub kind: SortKind,
}

/// ```text
/// 責務: [
/// RowOrderPlan: 並び替え後のデータ行の並びを保持する
/// ]
/// フィールド: [
/// keys: 計画に使ったSort key（先頭が最優先）
/// order: 新しい並びの各位置にくる元の行番号（データ行だけ。1以上）
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RowOrderPlan {
    keys: Vec<SortKey>,
    order: Vec<usize>,
}

impl RowOrderPlan {
    /// ```text
    /// 責務: [
    /// order: 新しい並びの元行番号を返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// &[usize]: 新しい位置（データ行の先頭から）ごとの元の行番号
    /// ]
    /// ```
    pub fn order(&self) -> &[usize] {
        &self.order
    }

    /// ```text
    /// 責務: [
    /// moved_count: 元の位置から動く行数を返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// usize: 位置が変わるデータ行の数（0なら適用しても何も変わらない）
    /// ]
    /// ```
    pub fn moved_count(&self) -> usize {
        self.order
            .iter()
            .enumerate()
            .filter(|(position, row)| **row != position + 1)
            .count()
    }
}

/// ```text
/// 責務: [
/// reject_multi_source: 複数physical sourceにまたがる並び替えの反映を拒否する
/// ]
/// 引数: [
/// source_count: logical tableを構成するphysical sourceの数
/// ]
/// 戻り値: [
/// (): single source（1つ）の場合
/// ]
/// エラー: [
/// DocumentError: 2つ以上のsourceにまたがる場合。source内の並び替えか、行を明示的に移すCommandを使う
/// ]
/// ```
pub fn reject_multi_source(source_count: usize) -> Result<(), DocumentError> {
    if source_count > 1 {
        return Err(DocumentError::Edit(format!(
            "the view spans {source_count} physical sources; sort inside each source or use an explicit redistribution command"
        )));
    }
    Ok(())
}

// {
// 責務: [
// parse_number: cell値を有限の数値として読む。読めなければNoneを返す
// ]
// 引数: [
// cell: Option<&str>
// ]
// 戻り値: [
// Option<f64>: 前後の空白を除いて有限の数値として読めた値
// ]
// }
fn parse_number(cell: Option<&str>) -> Option<f64> {
    cell?.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

// {
// 責務: [
// compare_rows: 2つのデータ行をSort keyの順に比較する
// ]
// 引数: [
// document: &CsvDocument
// keys: &[SortKey]
// a: usize
// b: usize
// ]
// 戻り値: [
// Ordering: aがbより前なら Less。全keyが同順位なら Equal（stable sortが元の順を保つ）
// ]
// }
fn compare_rows(document: &CsvDocument, keys: &[SortKey], a: usize, b: usize) -> Ordering {
    for key in keys {
        let left = document.cell(a, key.column);
        let right = document.cell(b, key.column);
        let ordering = match key.kind {
            SortKind::Raw => {
                let base = left.unwrap_or_default().cmp(right.unwrap_or_default());
                match key.direction {
                    SortDirection::Ascending => base,
                    SortDirection::Descending => base.reverse(),
                }
            }
            SortKind::Number => match (parse_number(left), parse_number(right)) {
                (Some(x), Some(y)) => {
                    let base = x.total_cmp(&y);
                    match key.direction {
                        SortDirection::Ascending => base,
                        SortDirection::Descending => base.reverse(),
                    }
                }
                // 読めないcellは方向に関係なく末尾の未解決グループへ置く
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            },
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

impl CsvDocument {
    /// ```text
    /// 責務: [
    /// plan_sort_rows: Sort keyで全データ行を並べ替えた場合の行順を、documentを変更せずに計算する
    /// ]
    /// 処理: [
    /// 1: keyが1つ以上で、すべての列がtable内にあるか確認する
    /// 2: header行を除く全データ行（Filterで隠れた行も含む）をstable sortする
    /// ]
    /// 引数: [
    /// keys: Sort key（先頭が最優先）
    /// ]
    /// 戻り値: [
    /// RowOrderPlan: 新しい行順
    /// ]
    /// エラー: [
    /// DocumentError: keyが空、またはtable外の列を含む場合
    /// ]
    /// ```
    pub fn plan_sort_rows(&self, keys: &[SortKey]) -> Result<RowOrderPlan, DocumentError> {
        if keys.is_empty() {
            return Err(DocumentError::Edit(
                "at least one sort key is required to reorder rows".into(),
            ));
        }
        if let Some(key) = keys.iter().find(|key| key.column >= self.column_count()) {
            return Err(DocumentError::Edit(format!(
                "column {} is outside the existing CSV table",
                key.column
            )));
        }
        let mut order: Vec<usize> = (1..self.row_count()).collect();
        order.sort_by(|a, b| compare_rows(self, keys, *a, *b));
        Ok(RowOrderPlan {
            keys: keys.to_vec(),
            order,
        })
    }

    /// ```text
    /// 責務: [
    /// apply_row_order_plan: 並び替えの計画をdocumentへ原子的に適用し、1回のUndoにまとめる
    /// ]
    /// 処理: [
    /// 1: 同じkeyで計画し直し、計画後に行が変わっていないか確認する
    /// 2: 全行を新しい順に並べたtableへ置き換え、検証してから1件の履歴を記録する
    /// ]
    /// 引数: [
    /// plan: plan_sort_rowsで作った計画
    /// ]
    /// 戻り値: [
    /// usize: 位置が変わった行数
    /// ]
    /// 副作用: [
    /// 並びが変わるなら履歴1件とdirty状態が更新される。変わらないなら何も記録しない
    /// ]
    /// エラー: [
    /// DocumentError: 計画後にdocumentが変わった、またはvalidationに反する場合（何も変更しない）
    /// ]
    /// ```
    pub fn apply_row_order_plan(&mut self, plan: &RowOrderPlan) -> Result<usize, DocumentError> {
        let current = self.plan_sort_rows(&plan.keys)?;
        if &current != plan {
            return Err(DocumentError::Edit(
                "rows changed after the sort was planned; plan again".into(),
            ));
        }
        let moved = plan.moved_count();
        if moved == 0 {
            return Ok(0);
        }
        let mut rows: Vec<Vec<String>> = Vec::with_capacity(self.row_count());
        rows.push(
            self.rows()
                .next()
                .map(<[String]>::to_vec)
                .unwrap_or_default(),
        );
        let data: Vec<Vec<String>> = self.rows().skip(1).map(<[String]>::to_vec).collect();
        rows.extend(plan.order.iter().map(|old| data[old - 1].clone()));
        self.replace_contents(rows)?;
        Ok(moved)
    }

    /// ```text
    /// 責務: [
    /// sort_rows: 計画と適用を続けて行い、位置が変わった行数を返す
    /// ]
    /// 引数: [
    /// keys: Sort key
    /// ]
    /// 戻り値: [
    /// usize: 位置が変わった行数
    /// ]
    /// エラー: [
    /// DocumentError: keyが不正、またはvalidationに反する場合（何も変更しない）
    /// ]
    /// ```
    pub fn sort_rows(&mut self, keys: &[SortKey]) -> Result<usize, DocumentError> {
        let plan = self.plan_sort_rows(keys)?;
        self.apply_row_order_plan(&plan)
    }
}

#[cfg(test)]
mod tests;
