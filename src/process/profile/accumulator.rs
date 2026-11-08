//! ```text
//! 責務: [
//! accumulator: 列のraw値を1件ずつ受け取り、全値をcloneせずにprofile統計を積み上げる
//! ]
//! 処理: [
//! 1: 値ごとに件数・文字数・空欄数と、明示型のparse結果を更新する
//! 2: 異なる値は件数表だけに持ち、上限を超えたら新しい値を数えず「計測省略」にする
//! 3: 別sourceのaccumulatorをmergeして、multi-sourceの全体統計を作る
//! ]
//! 補足: [
//! 型は呼び出し側が明示したものだけを使い、値から型を推論しない
//! 空欄はparse failureに含めず、empty countだけで数える
//! ]
//! ```
use std::cmp::Ordering;
use std::collections::HashMap;

use super::{ColumnProfile, FrequentValue, NumericRange};
use crate::process::ColumnType;

// {
// 責務: [
// Number: 明示型の最小・最大比較に使う数値を表す
// ]
// フィールド: [
// Int: 整数
// Dec: 有限の小数
// ]
// }
#[derive(Clone, Copy, Debug)]
enum Number {
    Int(i64),
    Dec(f64),
}

impl Number {
    // {
    // 責務: [
    // compare: 同じ種類の数値を比較する
    // ]
    // 引数: [
    // other: &Number
    // ]
    // 戻り値: [
    // Ordering: 順序（種類が違う場合はEqual。1つのaccumulatorでは同じ種類しか入らない）
    // ]
    // }
    fn compare(&self, other: &Number) -> Ordering {
        match (self, other) {
            (Number::Int(a), Number::Int(b)) => a.cmp(b),
            (Number::Dec(a), Number::Dec(b)) => a.total_cmp(b),
            _ => Ordering::Equal,
        }
    }
}

/// ```text
/// 責務: [
/// ProfileAccumulator: 1列分のprofile統計を積み上げ、別sourceの結果とmergeできる
/// ]
/// フィールド: [
/// column_type: 呼び出し側が明示した列型（Noneなら文字列統計だけ）
/// distinct_limit: 数える異なる値の上限
/// rows: 数えたcell数
/// empty: 空文字のcell数
/// min_length: 最短文字数
/// max_length: 最長文字数
/// counts: 値ごとの出現数（上限まで）
/// overflowed: 異なる値が上限を超えて、distinctを数え切れていない
/// parse_failures: 空でないのに明示型として読めなかった数
/// min: 明示数値型の最小値（数値と元のraw文字列）
/// max: 明示数値型の最大値
/// true_count: Booleanのtrue数
/// false_count: Booleanのfalse数
/// ]
/// ```
#[derive(Clone, Debug)]
pub struct ProfileAccumulator {
    column_type: Option<ColumnType>,
    distinct_limit: usize,
    rows: usize,
    empty: usize,
    min_length: Option<usize>,
    max_length: Option<usize>,
    counts: HashMap<String, usize>,
    overflowed: bool,
    parse_failures: usize,
    min: Option<(Number, String)>,
    max: Option<(Number, String)>,
    true_count: usize,
    false_count: usize,
}

impl ProfileAccumulator {
    /// ```text
    /// 責務: [
    /// new: 空のaccumulatorを作る
    /// ]
    /// 引数: [
    /// column_type: 明示された列型（Noneなら型統計を取らない）
    /// distinct_limit: 数える異なる値の上限
    /// ]
    /// 戻り値: [
    /// ProfileAccumulator: 何も数えていない状態
    /// ]
    /// ```
    pub fn new(column_type: Option<ColumnType>, distinct_limit: usize) -> Self {
        Self {
            column_type,
            distinct_limit,
            rows: 0,
            empty: 0,
            min_length: None,
            max_length: None,
            counts: HashMap::new(),
            overflowed: false,
            parse_failures: 0,
            min: None,
            max: None,
            true_count: 0,
            false_count: 0,
        }
    }

    /// ```text
    /// 責務: [
    /// add: cellのraw値を1件加える
    /// ]
    /// 引数: [
    /// value: cellのraw文字列
    /// ]
    /// 戻り値: []
    /// 副作用: [
    /// 件数・文字数・型統計が更新される
    /// ]
    /// ```
    pub fn add(&mut self, value: &str) {
        self.rows += 1;
        let length = value.chars().count();
        self.min_length = Some(self.min_length.map_or(length, |m| m.min(length)));
        self.max_length = Some(self.max_length.map_or(length, |m| m.max(length)));
        if let Some(count) = self.counts.get_mut(value) {
            *count += 1;
        } else if self.counts.len() < self.distinct_limit {
            self.counts.insert(value.to_owned(), 1);
        } else {
            self.overflowed = true;
        }
        if value.is_empty() {
            self.empty += 1;
            return;
        }
        match self.column_type {
            None | Some(ColumnType::String) => {}
            Some(ColumnType::Integer) => match value.parse::<i64>() {
                Ok(parsed) => self.track(Number::Int(parsed), value),
                Err(_) => self.parse_failures += 1,
            },
            Some(ColumnType::Decimal) => match value.parse::<f64>() {
                Ok(parsed) if parsed.is_finite() => self.track(Number::Dec(parsed), value),
                _ => self.parse_failures += 1,
            },
            Some(ColumnType::Boolean) => {
                if value.eq_ignore_ascii_case("true") {
                    self.true_count += 1;
                } else if value.eq_ignore_ascii_case("false") {
                    self.false_count += 1;
                } else {
                    self.parse_failures += 1;
                }
            }
        }
    }

    // {
    // 責務: [
    // track: 読めた数値で最小・最大を更新する
    // ]
    // 引数: [
    // number: Number
    // raw: &str
    // ]
    // 戻り値: []
    // }
    fn track(&mut self, number: Number, raw: &str) {
        if self
            .min
            .as_ref()
            .is_none_or(|(m, _)| number.compare(m) == Ordering::Less)
        {
            self.min = Some((number, raw.to_owned()));
        }
        if self
            .max
            .as_ref()
            .is_none_or(|(m, _)| number.compare(m) == Ordering::Greater)
        {
            self.max = Some((number, raw.to_owned()));
        }
    }

    /// ```text
    /// 責務: [
    /// merge: 別sourceのaccumulatorの統計を取り込む
    /// ]
    /// 引数: [
    /// other: 同じ列型で作ったaccumulator
    /// ]
    /// 戻り値: []
    /// 副作用: [
    /// 件数・最小最大・値の件数表が合算される。合算後に上限を超えたら計測省略になる
    /// ]
    /// ```
    pub fn merge(&mut self, other: &ProfileAccumulator) {
        self.rows += other.rows;
        self.empty += other.empty;
        self.parse_failures += other.parse_failures;
        self.true_count += other.true_count;
        self.false_count += other.false_count;
        self.min_length = match (self.min_length, other.min_length) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.max_length = match (self.max_length, other.max_length) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        self.overflowed |= other.overflowed;
        for (value, count) in &other.counts {
            if let Some(existing) = self.counts.get_mut(value) {
                *existing += count;
            } else if self.counts.len() < self.distinct_limit {
                self.counts.insert(value.clone(), *count);
            } else {
                self.overflowed = true;
            }
        }
        if let Some((number, raw)) = &other.min {
            self.track(*number, raw);
        }
        if let Some((number, raw)) = &other.max {
            self.track(*number, raw);
        }
    }

    /// ```text
    /// 責務: [
    /// finish: 積み上げた統計をColumnProfileにまとめる
    /// ]
    /// 引数: [
    /// column: 対象列（0始まり）
    /// top_values: 返す頻出値の数
    /// validation_violations: 同じscopeの入力規則違反数
    /// ]
    /// 戻り値: [
    /// ColumnProfile: 統計。計測省略のときdistinct・duplicate・頻出値は出さない
    /// ]
    /// ```
    pub fn finish(
        &self,
        column: usize,
        top_values: usize,
        validation_violations: usize,
    ) -> ColumnProfile {
        let exact = !self.overflowed;
        let mut frequent: Vec<FrequentValue> = if exact {
            self.counts
                .iter()
                .map(|(value, count)| FrequentValue {
                    value: value.clone(),
                    count: *count,
                })
                .collect()
        } else {
            Vec::new()
        };
        // 件数の多い順、同数ならraw値の順にして、実行のたびに揺れないようにする
        frequent.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
        frequent.truncate(top_values);
        let typed = matches!(
            self.column_type,
            Some(ColumnType::Integer | ColumnType::Decimal | ColumnType::Boolean)
        );
        ColumnProfile {
            column,
            rows: self.rows,
            empty: self.empty,
            non_empty: self.rows - self.empty,
            distinct: exact.then_some(self.counts.len()),
            duplicate: exact.then_some(self.rows - self.counts.len()),
            frequent,
            min_length: self.min_length,
            max_length: self.max_length,
            column_type: self.column_type,
            parse_failures: typed.then_some(self.parse_failures),
            numeric_range: match (&self.min, &self.max) {
                (Some((_, min)), Some((_, max))) => Some(NumericRange {
                    min: min.clone(),
                    max: max.clone(),
                }),
                _ => None,
            },
            boolean_counts: (self.column_type == Some(ColumnType::Boolean))
                .then_some((self.true_count, self.false_count)),
            validation_violations,
        }
    }
}
