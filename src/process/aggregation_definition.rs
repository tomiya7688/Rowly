//! ```text
//! 責務: [aggregation_definition: Group By / Pivotの定義を列見出しで持ち、現在のCSVの列へ解決する。保存形式（generated init DSL）はproject_init側が扱う]
//! 処理: [1: 定義は列index でなく見出し名で持つ 2: 解決時に見出しが見つからない・重複する定義は、その定義だけ休眠（dormant）にして理由を返す 3: 解決済みの要求はGroup By / Pivotコアへそのまま渡せる]
//! 補足: [定義は元CSVを変更しない。休眠した定義は破棄せず保持し、列が戻れば再び有効になる]
//! ```
use super::aggregation::{AggregateSpec, GroupByRequest};
use super::calculation::AggregateFunction;
use super::pivot::{PivotFilter, PivotRequest};

/// 集計の入力範囲。実際のrow選択は呼び出し側が行う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputScope {
    /// canonicalな全row。
    AllRows,
    /// 現在のView filter後のrow。
    ViewFiltered,
    /// 選択中のrow。
    Selection,
}

/// 見出しで指す集計列の定義。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregateDefinition {
    /// COUNT(*)。
    CountRows,
    /// 指定見出しの列へ集計関数を適用する。
    Column {
        /// 適用する関数。
        function: AggregateFunction,
        /// 対象列の見出し。
        header: String,
    },
    /// 指定見出しの列の、空でないraw値の種類数。
    DistinctCount {
        /// 対象列の見出し。
        header: String,
    },
}

/// Pivotの入力filter。見出しで列を指す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterDefinition {
    /// 判定する列の見出し。
    pub header: String,
    /// 残すraw値。
    pub allowed: Vec<String>,
}

/// 定義の種類。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregationKind {
    /// Group By。
    GroupBy {
        /// group keyにする見出し。
        keys: Vec<String>,
        /// 集計列。
        aggregates: Vec<AggregateDefinition>,
    },
    /// Pivot。
    Pivot {
        /// Rowsの見出し。
        rows: Vec<String>,
        /// Columnsの見出し。
        columns: Vec<String>,
        /// Values。
        values: Vec<AggregateDefinition>,
        /// 入力filter。
        filters: Vec<FilterDefinition>,
    },
}

/// 名前を持つ1つのGroup By / Pivot定義。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregationDefinition {
    /// 同じsource内で一意な名前。
    pub name: String,
    /// 入力範囲。
    pub scope: InputScope,
    /// 定義の中身。
    pub kind: AggregationKind,
}

/// 解決済みの要求。Group By / Pivotコアへそのまま渡せる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedRequest {
    /// Group Byの要求。
    GroupBy(GroupByRequest),
    /// Pivotの要求。
    Pivot(PivotRequest),
}

/// 休眠した定義の理由。休眠は定義を捨てず、その定義だけを使えなくする。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DormantReason {
    /// 現在のCSVに存在しない見出し。
    pub missing: Vec<String>,
    /// 現在のCSVに複数ある見出し。どの列か決められない。
    pub ambiguous: Vec<String>,
}

impl AggregationDefinition {
    // {
    //   責務: [resolve: 見出しを現在のCSVの列indexへ解決する]
    //   処理: [使う見出しをすべて探し、1つでも無い・重複するなら要求は作らずDormantReasonを返す。他の定義には影響しない]
    //   引数: [headers: 現在のCSVの見出し行]
    //   戻り値: [Result<ResolvedRequest, DormantReason>: 有効なら要求、休眠なら理由]
    // }
    pub fn resolve(&self, headers: &[String]) -> Result<ResolvedRequest, DormantReason> {
        let mut resolver = Resolver {
            headers,
            reason: DormantReason::default(),
        };
        let request = match &self.kind {
            AggregationKind::GroupBy { keys, aggregates } => {
                ResolvedRequest::GroupBy(GroupByRequest {
                    key_columns: resolver.columns(keys),
                    aggregates: resolver.aggregates(aggregates),
                })
            }
            AggregationKind::Pivot {
                rows,
                columns,
                values,
                filters,
            } => ResolvedRequest::Pivot(PivotRequest {
                row_columns: resolver.columns(rows),
                column_columns: resolver.columns(columns),
                values: resolver.aggregates(values),
                filters: filters
                    .iter()
                    .map(|filter| PivotFilter {
                        column: resolver.column(&filter.header),
                        allowed: filter.allowed.clone(),
                    })
                    .collect(),
            }),
        };
        if resolver.reason == DormantReason::default() {
            Ok(request)
        } else {
            Err(resolver.reason)
        }
    }
}

/// 見出しの解決中に、見つからない・重複する見出しを集める。
struct Resolver<'a> {
    headers: &'a [String],
    reason: DormantReason,
}

impl Resolver<'_> {
    // {
    //   責務: [column: 見出しの列indexを返す。見つからない・重複する見出しは理由へ記録し、ダミーの0を返す（要求は使われない）]
    // }
    fn column(&mut self, header: &str) -> usize {
        let mut found = self
            .headers
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.as_str() == header);
        match (found.next(), found.next()) {
            (Some((index, _)), None) => index,
            (None, _) => {
                push_unique(&mut self.reason.missing, header);
                0
            }
            (Some(_), Some(_)) => {
                push_unique(&mut self.reason.ambiguous, header);
                0
            }
        }
    }

    fn columns(&mut self, headers: &[String]) -> Vec<usize> {
        headers.iter().map(|header| self.column(header)).collect()
    }

    fn aggregates(&mut self, definitions: &[AggregateDefinition]) -> Vec<AggregateSpec> {
        definitions
            .iter()
            .map(|definition| match definition {
                AggregateDefinition::CountRows => AggregateSpec::CountRows,
                AggregateDefinition::Column { function, header } => AggregateSpec::Column {
                    function: *function,
                    column: self.column(header),
                },
                AggregateDefinition::DistinctCount { header } => AggregateSpec::DistinctCount {
                    column: self.column(header),
                },
            })
            .collect()
    }
}

fn push_unique(list: &mut Vec<String>, header: &str) {
    if !list.iter().any(|existing| existing == header) {
        list.push(header.to_owned());
    }
}
