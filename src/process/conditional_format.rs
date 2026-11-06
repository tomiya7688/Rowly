//! ```text
//! 責務: [conditional_format: 条件に合うcellへ表示styleを重ねる条件付き書式のruleと、副作用のない判定を提供する]
//! 処理: [1: ruleは対象・値の取り方・条件（pure predicate）・style・stop_if_trueを持つ 2: 値はraw文字列、明示した型（数値・日付）での解釈、freshな計算結果のどれかから取る 3: ruleを入力順に評価し、合ったruleのstyleをView styleと同じ規則で重ねる 4: 評価できない場合（型に読めない・計算結果がstale / error / pending / 無し）は、合ったことにせず理由を診断として返す]
//! 補足: [評価はFile I/Oなどの副作用を持たず、CSVには触れない。stale / errorの計算結果を通常の値として評価せず、型に読めない文字列を0などに読み替えない。同じ入力なら常に同じ結果になる]
//! ```
use std::cmp::Ordering;

use super::aggregation::CellFreshness;
use super::display_format::{compare_canonical_decimals, parse_iso_date};
use super::view_style::{CellStyle, DormantRule, StyleTarget, resolve_target, target_is_valid};

/// 条件で見る値の取り方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSource {
    /// raw文字列そのまま（比較は文字列の辞書順）。
    Raw,
    /// rawを正規形の数値として明示的に解釈する。読めなければ評価できない（0にしない）。
    Number,
    /// rawを`YYYY-MM-DD`の日付として明示的に解釈する。
    Date,
    /// freshな計算結果を文字列として見る。
    DerivedText,
    /// freshな計算結果を正規形の数値として解釈する。
    DerivedNumber,
}

/// 比較演算子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    /// 等しい。
    Eq,
    /// 等しくない。
    Ne,
    /// 小さい。
    Lt,
    /// 以下。
    Le,
    /// 大きい。
    Gt,
    /// 以上。
    Ge,
}

/// 条件の右辺の値。値の取り方と型が合っていなければならない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    /// 文字列（`Raw`・`DerivedText`と比べる）。
    Text(String),
    /// 正規形の数値（`Number`・`DerivedNumber`と比べる）。
    Number(String),
    /// `YYYY-MM-DD`の日付（`Date`と比べる）。
    Date(String),
}

/// 副作用のない条件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    /// 値と右辺の比較。
    Compare {
        /// 演算子。
        op: CompareOp,
        /// 右辺。
        literal: Literal,
    },
    /// 両端を含む範囲。
    Between {
        /// 下端。
        low: Literal,
        /// 上端。
        high: Literal,
    },
    /// 文字列を含む（大文字小文字は区別する。文字列の値だけ）。
    Contains(String),
    /// 文字列で始まる（文字列の値だけ）。
    StartsWith(String),
    /// 空（空白だけを含む）。
    IsEmpty,
    /// 空でない。
    IsNotEmpty,
    /// すべてに合う。
    And(Vec<Predicate>),
    /// どれかに合う。
    Or(Vec<Predicate>),
    /// 合わない。
    Not(Box<Predicate>),
}

/// 条件付き書式のrule。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalRule {
    /// 対象。View styleと同じtarget（列は見出し、行はCSVの行位置）。
    pub target: StyleTarget,
    /// 値の取り方。
    pub source: ValueSource,
    /// 条件。
    pub predicate: Predicate,
    /// 合ったときに重ねるstyle。
    pub style: CellStyle,
    /// 合ったら、これより後のruleを評価しない。
    pub stop_if_true: bool,
}

/// ruleの指定の不備。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConditionalError {
    /// targetが空、または範囲が逆。
    #[error("rule target is empty or its row range is reversed")]
    InvalidTarget,
    /// 右辺の値が、値の取り方に合わない（型違い、正規形でない数値、存在しない日付）。
    #[error("a literal does not match the value source")]
    LiteralMismatch,
    /// 条件が値の取り方に合わない、または空の組み合わせ・空の検索語。
    #[error("the predicate does not fit the value source or is empty")]
    InvalidPredicate,
}

/// 評価できなかった理由。合ったことにはならない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
    /// rawまたは計算結果が正規形の数値として読めない。
    NotANumber(String),
    /// rawまたは計算結果が日付として読めない。
    NotADate(String),
    /// 計算結果が無い。
    DerivedMissing,
    /// 計算結果がstale。
    DerivedStale,
    /// 計算結果がerror。
    DerivedError,
    /// 計算結果が評価待ち。
    DerivedPending,
}

/// 計算結果の入力。
#[derive(Debug, Clone, Copy)]
pub struct DerivedInput<'a> {
    /// 計算結果の文字列。
    pub text: &'a str,
    /// freshness。fresh以外は通常の値として評価しない。
    pub freshness: CellFreshness,
}

/// 1 cellの評価入力。
#[derive(Debug, Clone, Copy)]
pub struct CellInput<'a> {
    /// CSVのraw文字列。
    pub raw: &'a str,
    /// 計算結果。Calculation Bindingが無いcellはNone。
    pub derived: Option<DerivedInput<'a>>,
}

/// 条件付き書式。ruleの順序が優先順位になる。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConditionalFormat {
    /// rule。先のruleから評価する。
    pub rules: Vec<ConditionalRule>,
}

impl ConditionalFormat {
    // {
    //   責務: [add_rule: ruleを検証して末尾へ加える]
    //   処理: [targetが空・逆の範囲でないこと、右辺の型が値の取り方に合うこと、文字列の条件が文字列の値にだけ使われること、組み合わせ・検索語が空でないことを確かめる]
    // }
    pub fn add_rule(&mut self, rule: ConditionalRule) -> Result<(), ConditionalError> {
        if !target_is_valid(&rule.target) {
            return Err(ConditionalError::InvalidTarget);
        }
        validate_predicate(&rule.predicate, rule.source)?;
        self.rules.push(rule);
        Ok(())
    }

    // {
    //   責務: [resolve: 現在の見出しへruleのtargetを引き直す]
    //   処理: [View styleと同じ規則で対象の列を決める。見出しが無い・重複するruleだけを休眠にして理由を返し、他のruleは有効のまま。休眠したruleは捨てない]
    // }
    pub fn resolve(&self, headers: &[String]) -> ResolvedConditional {
        let mut active = Vec::new();
        let mut dormant = Vec::new();
        for (index, rule) in self.rules.iter().enumerate() {
            match resolve_target(&rule.target, headers) {
                Ok(target) => active.push(ActiveRule {
                    index,
                    columns: target.columns,
                    rows: target.rows,
                    rule: rule.clone(),
                }),
                Err(unresolved) => dormant.push(DormantRule {
                    index,
                    missing: unresolved.missing,
                    ambiguous: unresolved.ambiguous,
                }),
            }
        }
        ResolvedConditional { active, dormant }
    }
}

/// 現在の列へ解決できたrule。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveRule {
    /// 元のrule内での位置。
    pub index: usize,
    columns: Vec<usize>,
    rows: Option<(usize, usize)>,
    rule: ConditionalRule,
}

/// 解決済みの条件付き書式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConditional {
    /// 有効なrule（入力順）。
    pub active: Vec<ActiveRule>,
    /// 休眠したrule。
    pub dormant: Vec<DormantRule>,
}

/// 1 cellの評価結果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellEvaluation {
    /// 合ったruleのstyleを、先から順に重ねた結果。
    pub style: CellStyle,
    /// 合ったruleの元の位置（評価した順）。
    pub matched: Vec<usize>,
    /// 評価できなかったrule（元の位置と理由）。
    pub diagnostics: Vec<(usize, Diagnostic)>,
}

impl ResolvedConditional {
    // {
    //   責務: [evaluate: 1 cellに効く条件付き書式を、決定的に評価する]
    //   処理: [有効なruleを入力順にたどり、そのcellを対象にするruleだけ評価する。合ったらstyleを重ね、評価できなければ診断に残して合ったことにしない。stop_if_trueで合ったら残りを評価しない]
    //   引数: [row: CSVの行位置, column: 列位置, input: raw文字列と計算結果]
    // }
    pub fn evaluate(&self, row: usize, column: usize, input: &CellInput<'_>) -> CellEvaluation {
        let mut result = CellEvaluation::default();
        for active in &self.active {
            let applies = active.columns.contains(&column)
                && active
                    .rows
                    .is_none_or(|(first, last)| (first..=last).contains(&row));
            if !applies {
                continue;
            }
            match evaluate_rule(&active.rule, input) {
                Ok(true) => {
                    result.style = result.style.merged_with(&active.rule.style);
                    result.matched.push(active.index);
                    if active.rule.stop_if_true {
                        break;
                    }
                }
                Ok(false) => {}
                Err(diagnostic) => result.diagnostics.push((active.index, diagnostic)),
            }
        }
        result
    }
}

fn validate_literal(literal: &Literal, source: ValueSource) -> Result<(), ConditionalError> {
    let valid = match (literal, source) {
        (Literal::Text(_), ValueSource::Raw | ValueSource::DerivedText) => true,
        (Literal::Number(text), ValueSource::Number | ValueSource::DerivedNumber) => {
            compare_canonical_decimals(text, text).is_some()
        }
        (Literal::Date(text), ValueSource::Date) => parse_iso_date(text).is_some(),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ConditionalError::LiteralMismatch)
    }
}

fn validate_predicate(predicate: &Predicate, source: ValueSource) -> Result<(), ConditionalError> {
    let text_source = matches!(source, ValueSource::Raw | ValueSource::DerivedText);
    match predicate {
        Predicate::Compare { literal, .. } => validate_literal(literal, source),
        Predicate::Between { low, high } => {
            validate_literal(low, source)?;
            validate_literal(high, source)
        }
        Predicate::Contains(needle) | Predicate::StartsWith(needle) => {
            if text_source && !needle.is_empty() {
                Ok(())
            } else {
                Err(ConditionalError::InvalidPredicate)
            }
        }
        Predicate::IsEmpty | Predicate::IsNotEmpty => Ok(()),
        Predicate::And(items) | Predicate::Or(items) => {
            if items.is_empty() {
                return Err(ConditionalError::InvalidPredicate);
            }
            items
                .iter()
                .try_for_each(|item| validate_predicate(item, source))
        }
        Predicate::Not(inner) => validate_predicate(inner, source),
    }
}

// {
//   責務: [evaluate_rule: 1つのruleの条件を、値の取り方に従って取り出した値で評価する]
//   処理: [計算結果を見るruleは、計算結果が無い・freshでないときは値を評価せず理由を返す]
// }
fn evaluate_rule(rule: &ConditionalRule, input: &CellInput<'_>) -> Result<bool, Diagnostic> {
    let text = match rule.source {
        ValueSource::Raw | ValueSource::Number | ValueSource::Date => input.raw,
        ValueSource::DerivedText | ValueSource::DerivedNumber => match input.derived {
            None => return Err(Diagnostic::DerivedMissing),
            Some(derived) => match derived.freshness {
                CellFreshness::Fresh => derived.text,
                CellFreshness::Stale => return Err(Diagnostic::DerivedStale),
                CellFreshness::Error => return Err(Diagnostic::DerivedError),
                CellFreshness::Pending => return Err(Diagnostic::DerivedPending),
            },
        },
    };
    evaluate_predicate(&rule.predicate, rule.source, text)
}

fn evaluate_predicate(
    predicate: &Predicate,
    source: ValueSource,
    text: &str,
) -> Result<bool, Diagnostic> {
    match predicate {
        Predicate::Compare { op, literal } => {
            Ok(compare(source, text, literal)?.is_some_and(|ordering| holds(*op, ordering)))
        }
        Predicate::Between { low, high } => {
            let above = compare(source, text, low)?.is_some_and(|o| o != Ordering::Less);
            let below = compare(source, text, high)?.is_some_and(|o| o != Ordering::Greater);
            Ok(above && below)
        }
        Predicate::Contains(needle) => Ok(text.contains(needle.as_str())),
        Predicate::StartsWith(prefix) => Ok(text.starts_with(prefix.as_str())),
        Predicate::IsEmpty => Ok(text.trim().is_empty()),
        Predicate::IsNotEmpty => Ok(!text.trim().is_empty()),
        Predicate::And(items) => {
            let mut all = true;
            for item in items {
                all &= evaluate_predicate(item, source, text)?;
            }
            Ok(all)
        }
        Predicate::Or(items) => {
            let mut any = false;
            for item in items {
                any |= evaluate_predicate(item, source, text)?;
            }
            Ok(any)
        }
        Predicate::Not(inner) => Ok(!evaluate_predicate(inner, source, text)?),
    }
}

fn holds(op: CompareOp, ordering: Ordering) -> bool {
    match op {
        CompareOp::Eq => ordering == Ordering::Equal,
        CompareOp::Ne => ordering != Ordering::Equal,
        CompareOp::Lt => ordering == Ordering::Less,
        CompareOp::Le => ordering != Ordering::Greater,
        CompareOp::Gt => ordering == Ordering::Greater,
        CompareOp::Ge => ordering != Ordering::Less,
    }
}

// {
//   責務: [compare: 値と右辺を、値の取り方に従って比べる]
//   戻り値: [Result<Option<Ordering>, Diagnostic>: 空の値はNone（どの比較にも合わない）。型に読めない値はErr（0や空として扱わない）]
// }
fn compare(
    source: ValueSource,
    text: &str,
    literal: &Literal,
) -> Result<Option<Ordering>, Diagnostic> {
    match (source, literal) {
        (ValueSource::Raw | ValueSource::DerivedText, Literal::Text(other)) => {
            Ok(Some(text.cmp(other)))
        }
        (ValueSource::Number | ValueSource::DerivedNumber, Literal::Number(other)) => {
            if text.trim().is_empty() {
                return Ok(None);
            }
            compare_canonical_decimals(text, other)
                .map(Some)
                .ok_or_else(|| Diagnostic::NotANumber(text.to_owned()))
        }
        (ValueSource::Date, Literal::Date(other)) => {
            if text.trim().is_empty() {
                return Ok(None);
            }
            let value =
                parse_iso_date(text).ok_or_else(|| Diagnostic::NotADate(text.to_owned()))?;
            // 検証済みの右辺。add_ruleが存在する日付だけを通す。
            Ok(parse_iso_date(other).map(|other| value.cmp(&other)))
        }
        // add_ruleが型の合わない右辺を拒否するので、ここには来ない。
        _ => Ok(None),
    }
}
