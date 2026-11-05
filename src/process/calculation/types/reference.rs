//! 相対・絶対・混合のcell参照を表し、Fill時にrelative axisだけを移動する。

use std::{fmt, str::FromStr};

use crate::process::{CellRef, ReferenceError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [CalculationCellReference: row/columnごとに相対・絶対を持つcalculation式内のcell参照を表す]
/// 補足: [``A1`` は両軸相対、``$A$1`` は両軸絶対、``$A1`` はcolumn絶対、``A$1`` はrow絶対。式内だけの表記でCSVへは書かない]
/// ```
pub struct CalculationCellReference {
    /// 参照先のzero-based位置。
    cell: CellRef,
    /// rowがFillで移動しないならtrue。
    row_absolute: bool,
    /// columnがFillで移動しないならtrue。
    column_absolute: bool,
}

impl CalculationCellReference {
    /// {
    ///   責務: [new: 位置と各軸の絶対指定から参照を作る]
    ///   引数: [cell: 参照先, row_absolute: row絶対か, column_absolute: column絶対か]
    ///   戻り値: [Self: 参照]
    /// }
    pub const fn new(cell: CellRef, row_absolute: bool, column_absolute: bool) -> Self {
        Self {
            cell,
            row_absolute,
            column_absolute,
        }
    }

    /// {
    ///   責務: [cell: 参照先のzero-based位置を返す]
    ///   戻り値: [CellRef: 位置]
    /// }
    pub const fn cell(self) -> CellRef {
        self.cell
    }

    /// {
    ///   責務: [with_cell: 絶対指定を保ったまま参照先だけを差し替える]
    ///   引数: [cell: 新しい参照先]
    ///   戻り値: [Self: 差し替え後の参照]
    /// }
    pub const fn with_cell(self, cell: CellRef) -> Self {
        Self::new(cell, self.row_absolute, self.column_absolute)
    }

    /// {
    ///   責務: [is_relative: 両軸とも相対（``$``なし）かを返す]
    ///   戻り値: [bool: 両軸相対ならtrue]
    /// }
    pub const fn is_relative(self) -> bool {
        !self.row_absolute && !self.column_absolute
    }

    /// {
    ///   責務: [shifted: Fill先に合わせて相対軸だけを移動した参照を返す]
    ///   引数: [row_offset: row移動量, column_offset: column移動量]
    ///   戻り値: [Option<Self>: 移動後も座標が有効な参照。overflowや負座標はNone]
    /// }
    pub fn shifted(self, row_offset: isize, column_offset: isize) -> Option<Self> {
        let row = if self.row_absolute {
            self.cell.row()
        } else {
            self.cell.row().checked_add_signed(row_offset)?
        };
        let column = if self.column_absolute {
            self.cell.column()
        } else {
            self.cell.column().checked_add_signed(column_offset)?
        };
        Some(Self::new(
            CellRef::new(row, column),
            self.row_absolute,
            self.column_absolute,
        ))
    }
}

impl FromStr for CalculationCellReference {
    type Err = ReferenceError;

    /// {
    ///   責務: [from_str: ``A1``・``$A$1``・``$A1``・``A$1`` を解析する]
    ///   処理: [``$``は列文字の直前と行番号の直前だけに許可し、残りをCellRefの規則で検証する]
    ///   引数: [input: 参照文字列]
    ///   戻り値: [Result<Self, ReferenceError>: 参照、形式不正はInvalidCell]
    /// }
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let trimmed = input.trim();
        let (column_absolute, rest) = match trimmed.strip_prefix('$') {
            Some(rest) => (true, rest),
            None => (false, trimmed),
        };
        let letters = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_alphabetic())
            .count();
        let (letter_part, after_letters) = rest.split_at(letters);
        let (row_absolute, digit_part) = match after_letters.strip_prefix('$') {
            Some(digits) => (true, digits),
            None => (false, after_letters),
        };
        let plain = format!("{letter_part}{digit_part}");
        let well_formed = !letter_part.is_empty()
            && !digit_part.is_empty()
            && !digit_part.contains('$')
            && !letter_part.contains('$');
        if !well_formed {
            return Err(ReferenceError::InvalidCell(input.to_owned()));
        }
        // 元の入力をerrorへ残し、``$``を除いた中間文字列を利用者へ見せない。
        let cell = plain
            .parse::<CellRef>()
            .map_err(|_| ReferenceError::InvalidCell(input.to_owned()))?;
        Ok(Self::new(cell, row_absolute, column_absolute))
    }
}

impl fmt::Display for CalculationCellReference {
    /// {
    ///   責務: [fmt: 絶対軸に``$``を付けたA1形式へ直列化する]
    ///   引数: [formatter: 書き込み先]
    ///   戻り値: [fmt::Result: 書き込み結果]
    /// }
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plain = self.cell.to_string();
        let letters = plain.bytes().take_while(u8::is_ascii_alphabetic).count();
        let (column, row) = plain.split_at(letters);
        write!(
            formatter,
            "{}{column}{}{row}",
            if self.column_absolute { "$" } else { "" },
            if self.row_absolute { "$" } else { "" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(text: &str) -> CalculationCellReference {
        text.parse().unwrap()
    }

    // {
    //   責務: [four_modes_parse_and_round_trip: 4種類の参照modeの解析とserializeの往復を検証する]
    //   戻り値: [(): 往復で表記が保たれ、不正な``$``配置が拒否されれば成功する]
    // }
    #[test]
    fn four_modes_parse_and_round_trip() {
        for text in ["A1", "$A$1", "$A1", "A$1", "$AB$12"] {
            assert_eq!(reference(text).to_string(), text);
        }
        assert_eq!(
            "$A1x".parse::<CalculationCellReference>(),
            Err(ReferenceError::InvalidCell("$A1x".to_owned()))
        );
        assert!(reference("A1").is_relative());
        assert!(!reference("$A1").is_relative());
        for bad in ["$", "A$", "$$A1", "A$$1", "1A", "$1", "A1$", "", "$A$1$"] {
            assert!(bad.parse::<CalculationCellReference>().is_err(), "{bad}");
        }
    }

    // {
    //   責務: [shift_moves_only_relative_axes: Fillで相対軸だけが移動することを検証する]
    //   戻り値: [(): 4種類のmodeが設計どおりに移動または固定されれば成功する]
    // }
    #[test]
    fn shift_moves_only_relative_axes() {
        let moved = |text: &str, rows, columns| {
            reference(text)
                .shifted(rows, columns)
                .map(|value| value.to_string())
        };
        assert_eq!(moved("A1", 1, 0), Some("A2".to_owned()));
        assert_eq!(moved("$A$1", 1, 1), Some("$A$1".to_owned()));
        assert_eq!(moved("$A1", 1, 1), Some("$A2".to_owned()));
        assert_eq!(moved("A$1", 1, 1), Some("B$1".to_owned()));
        assert_eq!(moved("A1", -1, 0), None);
        assert_eq!(moved("$A1", 0, -5), Some("$A1".to_owned()));
        assert_eq!(moved("A$1", -5, 0), Some("A$1".to_owned()));
    }
}
