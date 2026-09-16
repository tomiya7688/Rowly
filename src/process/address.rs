use std::{fmt, str::FromStr};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellRef {
    row: usize,
    column: usize,
}

impl CellRef {
    pub const fn new(row: usize, column: usize) -> Self {
        Self { row, column }
    }

    pub const fn row(self) -> usize {
        self.row
    }

    pub const fn column(self) -> usize {
        self.column
    }
}

impl FromStr for CellRef {
    type Err = ReferenceError;

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
pub struct CellRange {
    start: CellRef,
    end: CellRef,
}

impl CellRange {
    pub fn new(first: CellRef, second: CellRef) -> Self {
        Self {
            start: CellRef::new(
                first.row.min(second.row),
                first.column.min(second.column),
            ),
            end: CellRef::new(
                first.row.max(second.row),
                first.column.max(second.column),
            ),
        }
    }

    pub const fn start(self) -> CellRef {
        self.start
    }

    pub const fn end(self) -> CellRef {
        self.end
    }

    pub fn iter(self) -> impl Iterator<Item = CellRef> {
        (self.start.row..=self.end.row).flat_map(move |row| {
            (self.start.column..=self.end.column).map(move |column| CellRef::new(row, column))
        })
    }
}

impl FromStr for CellRange {
    type Err = ReferenceError;

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
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.start == self.end {
            write!(formatter, "{}", self.start)
        } else {
            write!(formatter, "{}:{}", self.start, self.end)
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ReferenceError {
    #[error("invalid cell reference `{0}`")]
    InvalidCell(String),

    #[error("invalid cell range `{0}`")]
    InvalidRange(String),

    #[error("cell reference is too large `{0}`")]
    Overflow(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
    fn rejects_invalid_cell_references() {
        for source in ["", "A", "1", "A0", "A-1", "1A", "A1B"] {
            assert!(source.parse::<CellRef>().is_err(), "{source}");
        }
    }

    #[test]
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
    fn single_cell_is_a_valid_range() {
        let range = "C3".parse::<CellRange>().unwrap();
        assert_eq!(range.start(), CellRef::new(2, 2));
        assert_eq!(range.end(), CellRef::new(2, 2));
        assert_eq!(range.to_string(), "C3");
    }
}
