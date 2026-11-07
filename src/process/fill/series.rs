//! ```text
//! 責務: [
//! series: Series生成の値の列（SeriesGenerator）と、整数・小数の等差数列を提供する
//! ]
//! 補足: [
//! 小数は浮動小数点を使わず、10進の整数演算で計算する。overflowと平易でない10進文字列は拒否する
//! ]
//! ```
use crate::process::DocumentError;

/// ```text
/// 責務: [
/// SeriesGenerator: Series生成の値の列を表す拡張境界
/// ]
/// 処理: [
/// 1: 先頭からindex番目の値をraw文字列で返す
/// ]
/// 補足: [
/// 整数・小数のほか、日付などは明示されたformat/parserを持つ実装を追加して使う
/// ]
/// ```
pub trait SeriesGenerator {
    /// ```text
    /// 責務: [
    /// value: index番目（0始まり）の値を返す
    /// ]
    /// 引数: [
    /// index: 先頭からの位置
    /// ]
    /// 戻り値: [
    /// String: 生成したraw値
    /// ]
    /// エラー: [
    /// DocumentError: 値が表現できない（overflowなど）場合
    /// ]
    /// ```
    fn value(&self, index: usize) -> Result<String, DocumentError>;
}

/// ```text
/// 責務: [
/// DecimalSeries: 開始値と増分から等差の10進数列を生成する
/// ]
/// フィールド: [
/// start: 開始値の仮数（10進の整数）
/// step: 増分の仮数
/// scale: 小数点以下の桁数（開始値と増分の大きい方）
/// ]
/// 補足: [
/// 出力は常にscale桁の小数で表す（開始1・増分0.5なら1.0, 1.5, 2.0）
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecimalSeries {
    start: i128,
    step: i128,
    scale: u32,
}

/// ```text
/// 責務: [
/// IntegerSeries: 開始値と増分から等差の整数列を生成する
/// ]
/// フィールド: [
/// inner: 小数点以下0桁のDecimalSeries
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegerSeries {
    inner: DecimalSeries,
}

// {
// 責務: [
// parse_decimal: 符号つき10進文字列を仮数と小数桁数へ分解する
// ]
// 引数: [
// text: &str 例: -12, 3.50
// ]
// 戻り値: [
// Result<(i128, u32), DocumentError>: 仮数と小数桁数。指数表記・空白・空の整数部/小数部は拒否する
// ]
// }
fn parse_decimal(text: &str) -> Result<(i128, u32), DocumentError> {
    let bad = || DocumentError::Edit(format!("`{text}` is not a plain decimal number"));
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (whole, fraction) = match body.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (body, ""),
    };
    let digits_only = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty()
        || !digits_only(whole)
        || !digits_only(fraction)
        || (body.contains('.') && fraction.is_empty())
        || whole.len() + fraction.len() > 30
    {
        return Err(bad());
    }
    let mantissa: i128 = format!("{whole}{fraction}").parse().map_err(|_| bad())?;
    let scale = u32::try_from(fraction.len()).map_err(|_| bad())?;
    Ok((if negative { -mantissa } else { mantissa }, scale))
}

// {
// 責務: [
// rescale: 仮数を大きい小数桁数へ合わせる
// ]
// 引数: [
// mantissa: i128
// from: u32 現在の桁数
// to: u32 目的の桁数（from以上）
// ]
// 戻り値: [
// Result<i128, DocumentError>: 合わせた仮数。overflowは拒否する
// ]
// }
fn rescale(mantissa: i128, from: u32, to: u32) -> Result<i128, DocumentError> {
    let factor = 10_i128
        .checked_pow(to - from)
        .ok_or_else(|| DocumentError::Edit("series value is too large".into()))?;
    mantissa
        .checked_mul(factor)
        .ok_or_else(|| DocumentError::Edit("series value is too large".into()))
}

impl DecimalSeries {
    /// ```text
    /// 責務: [
    /// new: 開始値と増分の文字列から10進数列を作る
    /// ]
    /// 引数: [
    /// start: 開始値（例: 1, 0.5, -3.25）
    /// step: 増分（0も可）
    /// ]
    /// 戻り値: [
    /// DecimalSeries: 10進数列
    /// ]
    /// エラー: [
    /// DocumentError: 平易な10進数でない、または大きすぎる場合
    /// ]
    /// ```
    pub fn new(start: &str, step: &str) -> Result<Self, DocumentError> {
        let (start, start_scale) = parse_decimal(start)?;
        let (step, step_scale) = parse_decimal(step)?;
        let scale = start_scale.max(step_scale);
        Ok(Self {
            start: rescale(start, start_scale, scale)?,
            step: rescale(step, step_scale, scale)?,
            scale,
        })
    }
}

impl SeriesGenerator for DecimalSeries {
    fn value(&self, index: usize) -> Result<String, DocumentError> {
        let too_large = || DocumentError::Edit("series value is too large".into());
        let offset = self
            .step
            .checked_mul(i128::try_from(index).map_err(|_| too_large())?)
            .ok_or_else(too_large)?;
        let mantissa = self.start.checked_add(offset).ok_or_else(too_large)?;
        let digits = mantissa.unsigned_abs().to_string();
        let scale = self.scale as usize;
        let body = if scale == 0 {
            digits
        } else {
            let padded = format!("{digits:0>width$}", width = scale + 1);
            let (whole, fraction) = padded.split_at(padded.len() - scale);
            format!("{whole}.{fraction}")
        };
        Ok(if mantissa < 0 {
            format!("-{body}")
        } else {
            body
        })
    }
}

impl IntegerSeries {
    /// ```text
    /// 責務: [
    /// new: 開始値と増分の文字列から整数列を作る
    /// ]
    /// 引数: [
    /// start: 開始値（小数点を含めない）
    /// step: 増分（小数点を含めない）
    /// ]
    /// 戻り値: [
    /// IntegerSeries: 整数列
    /// ]
    /// エラー: [
    /// DocumentError: 整数でない、または大きすぎる場合
    /// ]
    /// ```
    pub fn new(start: &str, step: &str) -> Result<Self, DocumentError> {
        if start.contains('.') || step.contains('.') {
            return Err(DocumentError::Edit(
                "an integer series must not contain a decimal point".into(),
            ));
        }
        Ok(Self {
            inner: DecimalSeries::new(start, step)?,
        })
    }
}

impl SeriesGenerator for IntegerSeries {
    fn value(&self, index: usize) -> Result<String, DocumentError> {
        self.inner.value(index)
    }
}
