use std::{fmt, str::FromStr};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// ```text
/// 責務: [
/// CellRef: CSV上のcell位置をzero-based row/columnで保持する
/// ]
/// フィールド: [
/// row: cellのzero-based row index
/// column: cellのzero-based column index
/// ]
/// ```
pub struct CellRef {
    row: usize,
    column: usize,
}

impl CellRef {
    /// ```text
    /// 責務: [
    /// new: rowとcolumnからcell参照を作る
    /// ]
    /// 処理: [
    /// 1: 指定したindexをCellRefへ保持する
    /// ]
    /// 引数: [
    /// row: zero-based row index
    /// column: zero-based column index
    /// ]
    /// 戻り値: [
    /// CellRef: 指定位置を保持するcell参照
    /// ]
    /// ```
    pub const fn new(row: usize, column: usize) -> Self {
        Self { row, column }
    }

    /// ```text
    /// 責務: [
    /// row: cellのzero-based row indexを返す
    /// ]
    /// 処理: [
    /// 1: 保持しているrow indexを返す
    /// ]
    /// 引数: [
    /// self: 参照するcell位置
    /// ]
    /// 戻り値: [
    /// usize: zero-based row index
    /// ]
    /// ```
    pub const fn row(self) -> usize {
        self.row
    }

    /// ```text
    /// 責務: [
    /// column: cellのzero-based column indexを返す
    /// ]
    /// 処理: [
    /// 1: 保持しているcolumn indexを返す
    /// ]
    /// 引数: [
    /// self: 参照するcell位置
    /// ]
    /// 戻り値: [
    /// usize: zero-based column index
    /// ]
    /// ```
    pub const fn column(self) -> usize {
        self.column
    }
}

impl FromStr for CellRef {
    type Err = ReferenceError;

    // ```text
    // 責務: [
    // from_str: A1形式のcell参照をzero-based位置へ変換する
    // ]
    // 処理: [
    // 1: 空白を除き、列文字と行番号の形式を検証する
    // 2: 列文字をbase-26、行番号をusizeとして解析する
    // 3: 1-based座標をzero-based座標へ変換する
    // ]
    // 引数: [
    // input: A1形式のcell参照
    // ]
    // 戻り値: [
    // Result<CellRef, ReferenceError>: 解析した位置、または形式・範囲エラー
    // ]
    // ```
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.is_empty() {
            return Err(ReferenceError::InvalidCell(input.to_owned()));
        }

        let letter_count = input
            .bytes()
            .take_while(|byte| byte.is_ascii_alphabetic())
            .count();

        if letter_count == 0 || letter_count == input.len() {
            return Err(ReferenceError::InvalidCell(input.to_owned()));
        }

        let (letters, digits) = input.split_at(letter_count);
        if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ReferenceError::InvalidCell(input.to_owned()));
        }

        let mut column_number = 0usize;
        for byte in letters.bytes() {
            let digit = usize::from(byte.to_ascii_uppercase() - b'A' + 1);
            column_number = column_number
                .checked_mul(26)
                .and_then(|value| value.checked_add(digit))
                .ok_or_else(|| ReferenceError::Overflow(input.to_owned()))?;
        }

        let row_number = digits
            .parse::<usize>()
            .map_err(|_| ReferenceError::Overflow(input.to_owned()))?;
        if row_number == 0 {
            return Err(ReferenceError::InvalidCell(input.to_owned()));
        }

        Ok(Self::new(row_number - 1, column_number - 1))
    }
}

impl fmt::Display for CellRef {
    // ```text
    // 責務: [
    // fmt: zero-based cell位置をA1形式で出力する
    // ]
    // 処理: [
    // 1: 列番号をbase-26の英字へ変換する
    // 2: 行と列を1-based表記でformatterへ書き込む
    // ]
    // 引数: [
    // self: 出力するcell位置
    // formatter: 出力先formatter
    // ]
    // 戻り値: [
    // fmt::Result: 書き込み結果
    // ]
    // ```
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut number = self.column + 1;
        let mut letters = Vec::new();
        while number > 0 {
            let remainder = (number - 1) % 26;
            letters.push((b'A' + remainder as u8) as char);
            number = (number - 1) / 26;
        }
        letters.reverse();

        for letter in letters {
            formatter.write_str(&letter.to_string())?;
        }
        write!(formatter, "{}", self.row + 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [
/// CellRange: CSV上の矩形cell範囲を正規化した両端で保持する
/// ]
/// フィールド: [
/// start: 各軸で小さい方のcell位置
/// end: 各軸で大きい方のcell位置
/// ]
/// ```
pub struct CellRange {
    start: CellRef,
    end: CellRef,
}

impl CellRange {
    /// ```text
    /// 責務: [
    /// new: 2点から順序を正規化した矩形範囲を作る
    /// ]
    /// 処理: [
    /// 1: row/columnごとに小さい端点をstart、大きい端点をendへ設定する
    /// ]
    /// 引数: [
    /// first: 範囲の一端
    /// second: 範囲のもう一端
    /// ]
    /// 戻り値: [
    /// CellRange: row/columnごとに昇順の範囲
    /// ]
    /// ```
    pub fn new(first: CellRef, second: CellRef) -> Self {
        Self {
            start: CellRef::new(first.row.min(second.row), first.column.min(second.column)),
            end: CellRef::new(first.row.max(second.row), first.column.max(second.column)),
        }
    }

    /// ```text
    /// 責務: [
    /// start: 範囲の開始cellを返す
    /// ]
    /// 処理: [
    /// 1: 保持している開始位置を返す
    /// ]
    /// 処理: [
    /// 1: 保持している開始位置を返す
    /// ]
    /// 引数: [
    /// self: 対象範囲
    /// ]
    /// 戻り値: [
    /// CellRef: row/columnごとに小さい側のcell
    /// ]
    /// ```
    pub const fn start(self) -> CellRef {
        self.start
    }

    /// ```text
    /// 責務: [
    /// end: 範囲の終了cellを返す
    /// ]
    /// 処理: [
    /// 1: 保持している終了位置を返す
    /// ]
    /// 処理: [
    /// 1: 保持している終了位置を返す
    /// ]
    /// 引数: [
    /// self: 対象範囲
    /// ]
    /// 戻り値: [
    /// CellRef: row/columnごとに大きい側のcell
    /// ]
    /// ```
    pub const fn end(self) -> CellRef {
        self.end
    }

    /// ```text
    /// 責務: [
    /// iter: 範囲内のcellをrow-major順で列挙する
    /// ]
    /// 処理: [
    /// 1: 各行を順に進み、行内の各columnからCellRefを生成する
    /// ]
    /// 引数: [
    /// self: 列挙する矩形範囲
    /// ]
    /// 戻り値: [
    /// impl Iterator<Item = CellRef>: 行ごとに左から右へ進むcell iterator
    /// ]
    /// ```
    pub fn iter(self) -> impl Iterator<Item = CellRef> {
        (self.start.row..=self.end.row).flat_map(move |row| {
            (self.start.column..=self.end.column).map(move |column| CellRef::new(row, column))
        })
    }
}

impl FromStr for CellRange {
    type Err = ReferenceError;

    // ```text
    // 責務: [
    // from_str: A1またはA1:B2形式の範囲を解析する
    // ]
    // 処理: [
    // 1: 空白を除き、区切りが0個または1個であることを確認する
    // 2: 両端をCellRefとして解析する
    // 3: 単一cellを許可し、複数cell範囲は端点を正規化する
    // ]
    // 引数: [
    // input: cell参照または範囲文字列
    // ]
    // 戻り値: [
    // Result<CellRange, ReferenceError>: 正規化範囲、または形式・座標エラー
    // ]
    // ```
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        let mut parts = input.split(':');
        let first = parts
            .next()
            .ok_or_else(|| ReferenceError::InvalidRange(input.to_owned()))?;
        let second = parts.next();
        if parts.next().is_some() {
            return Err(ReferenceError::InvalidRange(input.to_owned()));
        }

        let first = first.parse::<CellRef>()?;
        let second = match second {
            Some(value) if !value.trim().is_empty() => value.parse::<CellRef>()?,
            Some(_) => return Err(ReferenceError::InvalidRange(input.to_owned())),
            None => first,
        };

        Ok(Self::new(first, second))
    }
}

impl fmt::Display for CellRange {
    // ```text
    // 責務: [
    // fmt: 範囲をA1形式で出力し、単一cellなら端点を省略する
    // ]
    // 処理: [
    // 1: 単一cellなら1参照、範囲なら開始と終了を区切り文字で出力する
    // ]
    // 引数: [
    // self: 出力する範囲
    // formatter: 出力先formatter
    // ]
    // 戻り値: [
    // fmt::Result: 書き込み結果
    // ]
    // ```
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.start == self.end {
            write!(formatter, "{}", self.start)
        } else {
            write!(formatter, "{}:{}", self.start, self.end)
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ReferenceError: cell参照と範囲の解析で発生する入力・数値範囲エラーを表す
/// ]
/// 補足: [
/// InvalidCell: A1形式のcell参照が不正
/// InvalidRange: 範囲区切りまたは範囲端点が不正
/// Overflow: 列・行の数値がusizeで表現できない
/// ]
/// ```
pub enum ReferenceError {
    #[error("invalid cell reference `{0}`")]
    InvalidCell(String),

    #[error("invalid cell range `{0}`")]
    InvalidRange(String),

    #[error("cell reference is too large `{0}`")]
    Overflow(String),
}

#[cfg(test)]
// ```text
// 責務: [
// tests: cell参照と矩形範囲の解析・表示・列挙を検証する
// ]
// ```
mod tests {
    use super::*;

    #[test]
    // ```text
    // 責務: [
    // parses_and_formats_a1_references: 複数列形式を含むA1変換を検証する
    // ]
    // 処理: [
    // 1: A1例をCellRefへ解析し、zero-based位置と再表示を照合する
    // ]
    // 引数: []
    // 戻り値: [(): 全assertion成功時に正常終了する]
    // ```
    fn parses_and_formats_a1_references() {
        for (source, row, column, formatted) in [
            ("A1", 0, 0, "A1"),
            ("z9", 8, 25, "Z9"),
            ("AA10", 9, 26, "AA10"),
            ("XFD1048576", 1_048_575, 16_383, "XFD1048576"),
        ] {
            let reference = source.parse::<CellRef>().unwrap();
            assert_eq!(reference, CellRef::new(row, column));
            assert_eq!(reference.to_string(), formatted);
        }
    }

    #[test]
    // ```text
    // 責務: [
    // rejects_invalid_cell_references: 空・欠落・不正な行列表記を拒否することを検証する
    // ]
    // 処理: [
    // 1: 不正例を順に解析し、全てerrorになることをassertする
    // ]
    // 引数: []
    // 戻り値: [(): 全assertion成功時に正常終了する]
    // ```
    fn rejects_invalid_cell_references() {
        for source in ["", "A", "1", "A0", "A-1", "1A", "A1B"] {
            assert!(source.parse::<CellRef>().is_err(), "{source}");
        }
    }

    #[test]
    // ```text
    // 責務: [
    // range_normalizes_reverse_corners_and_iterates_rectangle: 逆順端点を正規化し全cellを列挙することを検証する
    // ]
    // 処理: [
    // 1: 逆順rangeを解析し、正規化端点・cell順・表示をassertする
    // ]
    // 引数: []
    // 戻り値: [(): 全assertion成功時に正常終了する]
    // ```
    fn range_normalizes_reverse_corners_and_iterates_rectangle() {
        let range = "B2:A1".parse::<CellRange>().unwrap();

        assert_eq!(range.start(), CellRef::new(0, 0));
        assert_eq!(range.end(), CellRef::new(1, 1));
        assert_eq!(
            range.iter().collect::<Vec<_>>(),
            vec![
                CellRef::new(0, 0),
                CellRef::new(0, 1),
                CellRef::new(1, 0),
                CellRef::new(1, 1),
            ]
        );
        assert_eq!(range.to_string(), "A1:B2");
    }

    #[test]
    // ```text
    // 責務: [
    // single_cell_is_a_valid_range: 単一cellを範囲として解析・表示できることを検証する
    // ]
    // 処理: [
    // 1: 単一cell範囲の端点と表示結果をassertする
    // ]
    // 引数: []
    // 戻り値: [(): 全assertion成功時に正常終了する]
    // ```
    fn single_cell_is_a_valid_range() {
        let range = "C3".parse::<CellRange>().unwrap();
        assert_eq!(range.start(), CellRef::new(2, 2));
        assert_eq!(range.end(), CellRef::new(2, 2));
        assert_eq!(range.to_string(), "C3");
    }
}
