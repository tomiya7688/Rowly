use thiserror::Error;

use super::{CellRef, CsvDocument};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ColumnType: cell値を変更せずに判定する列データ型を表す
/// ]
/// 補足: [
/// String: 全ての文字列値を受理する
/// Integer: i64として解析できる値
/// Decimal: 有限のf64として解析できる値
/// Boolean: trueまたはfalseを大文字小文字を区別せず受理する
/// ]
/// ```
pub enum ColumnType {
    String,
    Integer,
    Decimal,
    Boolean,
}

impl ColumnType {
    // ```text
    // 責務: [
    // as_metadata_str: 列型をsidecar保存用の文字列へ変換する
    // ]
    // 処理: [
    // 1: 列型variantに対応する文字列を返す
    // ]
    // 引数: [
    // self: 保存する列型
    // ]
    // 戻り値: [
    // &'static str: 列型名
    // ]
    // ```
    pub(crate) fn as_metadata_str(self) -> &'static str {
        match self {
            Self::String => "String",
            Self::Integer => "Integer",
            Self::Decimal => "Decimal",
            Self::Boolean => "Boolean",
        }
    }

    // ```text
    // 責務: [
    // from_metadata_str: sidecar列型名をColumnTypeへ変換する
    // ]
    // 処理: [
    // 1: 対応する型名を照合し、認識できない場合はNoneを返す
    // ]
    // 引数: [
    // value: sidecarに保存された列型名
    // ]
    // 戻り値: [
    // Option<ColumnType>: 対応する列型、または未対応名の場合None
    // ]
    // ```
    pub(crate) fn from_metadata_str(value: &str) -> Option<Self> {
        match value {
            "String" => Some(Self::String),
            "Integer" => Some(Self::Integer),
            "Decimal" => Some(Self::Decimal),
            "Boolean" => Some(Self::Boolean),
            _ => None,
        }
    }

    // ```text
    // 責務: [
    // matches: 文字列cell値がこの列型として解析可能かを判定する
    // ]
    // 処理: [
    // 1: 型ごとのparserまたは文字列規則で値を判定する
    // ]
    // 引数: [
    // self: 判定に使う列型
    // value: 判定するCSV文字列値
    // ]
    // 戻り値: [
    // bool: 列型に適合する場合true
    // ]
    // ```
    fn matches(self, value: &str) -> bool {
        match self {
            Self::String => true,
            Self::Integer => value.parse::<i64>().is_ok(),
            Self::Decimal => value.parse::<f64>().is_ok_and(|parsed| parsed.is_finite()),
            Self::Boolean => {
                value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ColumnCell: 列チェック結果に含まれるcell参照と元の文字列値を保持する
/// ]
/// フィールド: [
/// reference: 結果cellのzero-based位置
/// value: 判定対象となったCSV文字列値
/// ]
/// ```
pub struct ColumnCell {
    reference: CellRef,
    value: String,
}

impl ColumnCell {
    /// ```text
    /// 責務: [
    /// reference: 結果cellの位置を返す
    /// ]
    /// 処理: [
    /// 1: 保持しているcell位置を返す
    /// ]
    /// 引数: [
    /// self: 対象cell結果
    /// ]
    /// 戻り値: [
    /// CellRef: cellのzero-based位置
    /// ]
    /// ```
    pub fn reference(&self) -> CellRef {
        self.reference
    }

    /// ```text
    /// 責務: [
    /// value: 結果cellの元の文字列値を返す
    /// ]
    /// 処理: [
    /// 1: 保持しているCSV文字列へのborrowを返す
    /// ]
    /// 引数: [
    /// self: 対象cell結果
    /// ]
    /// 戻り値: [
    /// &str: CSV内のcell文字列
    /// ]
    /// ```
    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ColumnTypeReport: 1列の型チェックで確認したcell数と不適合cellを保持する
/// ]
/// フィールド: [
/// column: 検証したzero-based column index
/// column_type: 適用した型
/// checked_cells: header以外で列内に存在するcell数。空文字cellも含む
/// mismatches: 型に適合しなかったcellと文字列値
/// ]
/// ```
pub struct ColumnTypeReport {
    column: usize,
    column_type: ColumnType,
    checked_cells: usize,
    mismatches: Vec<ColumnCell>,
}

impl ColumnTypeReport {
    /// ```text
    /// 責務: [
    /// column: 検証したzero-based column indexを返す
    /// ]
    /// 処理: [
    /// 1: 保持しているcolumn indexを返す
    /// ]
    /// 引数: [
    /// self: 対象レポート
    /// ]
    /// 戻り値: [
    /// usize: 検証対象column index
    /// ]
    /// ```
    pub fn column(&self) -> usize {
        self.column
    }

    /// ```text
    /// 責務: [
    /// column_type: 適用した列型を返す
    /// ]
    /// 処理: [
    /// 1: 保持している列型を返す
    /// ]
    /// 引数: [
    /// self: 対象レポート
    /// ]
    /// 戻り値: [
    /// ColumnType: 検証に使った型
    /// ]
    /// ```
    pub fn column_type(&self) -> ColumnType {
        self.column_type
    }

    /// ```text
    /// 責務: [
    /// checked_cells: 型判定したcell数（空文字cellを含む）を返す
    /// ]
    /// 処理: [
    /// 1: 保持している判定件数を返す
    /// ]
    /// 引数: [
    /// self: 対象レポート
    /// ]
    /// 戻り値: [
    /// usize: headerを除いて確認したcell数
    /// ]
    /// ```
    pub fn checked_cells(&self) -> usize {
        self.checked_cells
    }

    /// ```text
    /// 責務: [
    /// mismatches: 型に適合しなかったcell結果を返す
    /// ]
    /// 処理: [
    /// 1: 不適合cell sliceを借用して返す
    /// ]
    /// 引数: [
    /// self: 対象レポート
    /// ]
    /// 戻り値: [
    /// &[ColumnCell]: 不適合cellの参照と値
    /// ]
    /// ```
    pub fn mismatches(&self) -> &[ColumnCell] {
        &self.mismatches
    }

    /// ```text
    /// 責務: [
    /// is_valid: 型に適合しないcellがないか判定する
    /// ]
    /// 処理: [
    /// 1: 不適合一覧が空かを調べる
    /// ]
    /// 引数: [
    /// self: 対象レポート
    /// ]
    /// 戻り値: [
    /// bool: 不適合cellがない場合true
    /// ]
    /// ```
    pub fn is_valid(&self) -> bool {
        self.mismatches.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// JapaneseCheckReport: 1列の日本語文字チェックで一致・不一致cellを保持する
/// ]
/// フィールド: [
/// column: 検証したzero-based column index
/// checked_cells: header以外で列内に存在するcell数。空文字cellも含む
/// matches: 対象文字範囲に一致したcell
/// mismatches: 対象文字範囲に一致しなかったcell
/// ]
/// ```
pub struct JapaneseCheckReport {
    column: usize,
    checked_cells: usize,
    matches: Vec<ColumnCell>,
    mismatches: Vec<ColumnCell>,
}

impl JapaneseCheckReport {
    /// ```text
    /// 責務: [
    /// column: 検証したzero-based column indexを返す
    /// ]
    /// 処理: [
    /// 1: 保持しているcolumn indexを返す
    /// ]
    /// 引数: [
    /// self: 対象レポート
    /// ]
    /// 戻り値: [
    /// usize: 検証対象column index
    /// ]
    /// ```
    pub fn column(&self) -> usize {
        self.column
    }

    /// ```text
    /// 責務: [
    /// checked_cells: 日本語文字を判定したcell数（空文字cellを含む）を返す
    /// ]
    /// 処理: [
    /// 1: 保持している判定件数を返す
    /// ]
    /// 引数: [
    /// self: 対象レポート
    /// ]
    /// 戻り値: [
    /// usize: headerを除いて確認したcell数
    /// ]
    /// ```
    pub fn checked_cells(&self) -> usize {
        self.checked_cells
    }

    /// ```text
    /// 責務: [
    /// matches: 日本語文字を含むcell結果を返す
    /// ]
    /// 処理: [
    /// 1: 一致cell sliceを借用して返す
    /// ]
    /// 引数: [
    /// self: 対象レポート
    /// ]
    /// 戻り値: [
    /// &[ColumnCell]: 条件に一致したcellの位置と値
    /// ]
    /// ```
    pub fn matches(&self) -> &[ColumnCell] {
        &self.matches
    }

    /// ```text
    /// 責務: [
    /// mismatches: 日本語文字を含まないcell結果を返す
    /// ]
    /// 処理: [
    /// 1: 不一致cell sliceを借用して返す
    /// ]
    /// 引数: [
    /// self: 対象レポート
    /// ]
    /// 戻り値: [
    /// &[ColumnCell]: 条件に一致しなかったcellの位置と値
    /// ]
    /// ```
    pub fn mismatches(&self) -> &[ColumnCell] {
        &self.mismatches
    }

    /// ```text
    /// 責務: [
    /// all_match: 全確認値が一致し、確認対象も空でないかを判定する
    /// ]
    /// 処理: [
    /// 1: 判定件数が1件以上で不一致一覧が空かを確認する
    /// ]
    /// 引数: [
    /// self: 対象レポート
    /// ]
    /// 戻り値: [
    /// bool: 1件以上を確認し、不一致がない場合true
    /// ]
    /// ```
    pub fn all_match(&self) -> bool {
        self.checked_cells > 0 && self.mismatches.is_empty()
    }
}

impl CsvDocument {
    /// ```text
    /// 責務: [
    /// column_indices_by_header: 先頭行でheader名が一致する全column indexを返す
    /// ]
    /// 処理: [
    /// 1: 全columnを走査し、先頭行の値が完全一致するindexを集める
    /// ]
    /// 引数: [
    /// self: 検索対象document
    /// header: 完全一致で検索するheader文字列
    /// ]
    /// 戻り値: [
    /// Vec<usize>: 一致したzero-based column index。未検出なら空
    /// ]
    /// ```
    pub fn column_indices_by_header(&self, header: &str) -> Vec<usize> {
        (0..self.column_count())
            .filter(|&column| self.cell(0, column) == Some(header))
            .collect()
    }

    /// ```text
    /// 責務: [
    /// column_index_by_header: header名から一意なcolumn indexを取得する
    /// ]
    /// 処理: [
    /// 1: 一致なし・一件・複数件を区別してindexまたはerrorを返す
    /// ]
    /// 引数: [
    /// self: 検索対象document
    /// header: 完全一致で検索するheader文字列
    /// ]
    /// 戻り値: [
    /// Result<usize, ColumnError>: 一意なindex、未検出または重複headerのerror
    /// ]
    /// ```
    pub fn column_index_by_header(&self, header: &str) -> Result<usize, ColumnError> {
        let matches = self.column_indices_by_header(header);
        match matches.as_slice() {
            [column] => Ok(*column),
            [] => Err(ColumnError::HeaderNotFound(header.to_owned())),
            _ => Err(ColumnError::AmbiguousHeader {
                header: header.to_owned(),
                columns: matches,
            }),
        }
    }

    /// ```text
    /// 責務: [
    /// validate_column_type: headerを除く指定列の値を型判定し、不一致を報告する
    /// ]
    /// 処理: [
    /// 1: 指定columnの存在を確認する
    /// 2: header以外の全cell（空文字を含む）を型判定し、位置と元値を集める
    /// ]
    /// 引数: [
    /// self: 検証対象document
    /// column: zero-based column index
    /// column_type: 適用する判定型
    /// ]
    /// 戻り値: [
    /// Result<ColumnTypeReport, ColumnError>: 判定結果、または範囲外column error
    /// ]
    /// ```
    pub fn validate_column_type(
        &self,
        column: usize,
        column_type: ColumnType,
    ) -> Result<ColumnTypeReport, ColumnError> {
        self.ensure_column_exists(column)?;

        let mut checked_cells = 0;
        let mut mismatches = Vec::new();
        for row in 1..self.row_count() {
            let Some(value) = self.cell(row, column) else {
                continue;
            };
            checked_cells += 1;
            if !column_type.matches(value) {
                mismatches.push(ColumnCell {
                    reference: CellRef::new(row, column),
                    value: value.to_owned(),
                });
            }
        }

        Ok(ColumnTypeReport {
            column,
            column_type,
            checked_cells,
            mismatches,
        })
    }

    /// ```text
    /// 責務: [
    /// validate_column_type_by_header: headerで列を一意に特定し、値を型判定する
    /// ]
    /// 処理: [
    /// 1: header名からcolumn indexを解決する
    /// 2: 指定型でcolumn値を検証する
    /// ]
    /// 引数: [
    /// self: 検証対象document
    /// header: 検索するheader文字列
    /// column_type: 適用する判定型
    /// ]
    /// 戻り値: [
    /// Result<ColumnTypeReport, ColumnError>: 判定結果、未検出/重複header error
    /// ]
    /// ```
    pub fn validate_column_type_by_header(
        &self,
        header: &str,
        column_type: ColumnType,
    ) -> Result<ColumnTypeReport, ColumnError> {
        let column = self.column_index_by_header(header)?;
        self.validate_column_type(column, column_type)
    }

    /// ```text
    /// 責務: [
    /// check_column_japanese: headerを除く指定列の値を日本語文字範囲で分類する
    /// ]
    /// 処理: [
    /// 1: 指定columnの存在を確認する
    /// 2: header以外の全cell（空文字を含む）を検査し、一致と不一致に分ける
    /// ]
    /// 引数: [
    /// self: 検証対象document
    /// column: zero-based column index
    /// ]
    /// 戻り値: [
    /// Result<JapaneseCheckReport, ColumnError>: 分類結果、または範囲外column error
    /// ]
    /// ```
    pub fn check_column_japanese(&self, column: usize) -> Result<JapaneseCheckReport, ColumnError> {
        self.ensure_column_exists(column)?;

        let mut checked_cells = 0;
        let mut matches = Vec::new();
        let mut mismatches = Vec::new();
        for row in 1..self.row_count() {
            let Some(value) = self.cell(row, column) else {
                continue;
            };
            checked_cells += 1;
            let cell = ColumnCell {
                reference: CellRef::new(row, column),
                value: value.to_owned(),
            };
            if contains_japanese(value) {
                matches.push(cell);
            } else {
                mismatches.push(cell);
            }
        }

        Ok(JapaneseCheckReport {
            column,
            checked_cells,
            matches,
            mismatches,
        })
    }

    /// ```text
    /// 責務: [
    /// check_column_japanese_by_header: headerで列を一意に特定し、日本語文字を検査する
    /// ]
    /// 処理: [
    /// 1: header名からcolumn indexを解決する
    /// 2: 対象columnの値を日本語文字範囲で分類する
    /// ]
    /// 引数: [
    /// self: 検証対象document
    /// header: 検索するheader文字列
    /// ]
    /// 戻り値: [
    /// Result<JapaneseCheckReport, ColumnError>: 分類結果、未検出/重複header error
    /// ]
    /// ```
    pub fn check_column_japanese_by_header(
        &self,
        header: &str,
    ) -> Result<JapaneseCheckReport, ColumnError> {
        let column = self.column_index_by_header(header)?;
        self.check_column_japanese(column)
    }

    // ```text
    // 責務: [
    // ensure_column_exists: 指定zero-based column indexがdocument内にあるか確認する
    // ]
    // 処理: [
    // 1: indexをcolumn countと比較し、範囲外ならerrorにする
    // ]
    // 引数: [
    // self: 対象document
    // column: 確認するcolumn index
    // ]
    // 戻り値: [
    // Result<(), ColumnError>: 存在時は成功、範囲外なら列数を含むerror
    // ]
    // ```
    fn ensure_column_exists(&self, column: usize) -> Result<(), ColumnError> {
        let column_count = self.column_count();
        if column >= column_count {
            return Err(ColumnError::ColumnOutOfBounds {
                column,
                column_count,
            });
        }
        Ok(())
    }
}

/// ```text
/// 責務: [
/// contains_japanese: 文字列に対象の日本語Unicode文字が含まれるか判定する
/// ]
/// 処理: [
/// 1: 各Unicode文字をis_japanese_characterで調べる
/// ]
/// 引数: [
/// value: 検査する文字列
/// ]
/// 戻り値: [
/// bool: 対象範囲内の文字を含む場合true
/// ]
/// ```
pub fn contains_japanese(value: &str) -> bool {
    value.chars().any(is_japanese_character)
}

// ```text
// 責務: [
// is_japanese_character: 文字が判定対象の日本語Unicode範囲に入るか判定する
// ]
// 処理: [
// 1: code pointが列挙されたUnicode rangeのいずれかに入るか調べる
// ]
// 引数: [
// character: 検査するUnicode文字
// ]
// 戻り値: [
// bool: 対象文字範囲に含まれる場合true
// ]
// ```
fn is_japanese_character(character: char) -> bool {
    matches!(
        character as u32,
        0x3005..=0x3007
            | 0x3040..=0x309F
            | 0x30A0..=0x30FF
            | 0x31F0..=0x31FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xFF66..=0xFF9D
    )
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ColumnError: header列の解決とcolumn index検証で発生する失敗を表す
/// ]
/// 補足: [
/// HeaderNotFound: header名が見つからない
/// AmbiguousHeader: header名が複数columnで一致する
/// ColumnOutOfBounds: indexがdocumentの列数以上である
/// ]
/// ```
pub enum ColumnError {
    #[error("column header `{0}` was not found")]
    HeaderNotFound(String),

    #[error("column header `{header}` is ambiguous across columns {columns:?}")]
    AmbiguousHeader { header: String, columns: Vec<usize> },

    #[error("column {column} is out of bounds for {column_count} columns")]
    ColumnOutOfBounds { column: usize, column_count: usize },
}

#[cfg(test)]
// ```text
// 責務: [
// tests: header解決、列型判定、日本語文字検査の結果と非変更性を検証する
// ]
// ```
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    // ```text
    // 責務: [
    // open: テスト用文字列から一時CSVとCsvDocumentを作る
    // ]
    // 処理: [
    // 1: 一時directoryとCSVを作り、CsvDocument::openで読み込む
    // ]
    // 引数: [
    // source: 一時CSVへ書く内容
    // ]
    // 戻り値: [
    // CsvDocument: 一時CSVを開いたdocument
    // ]
    // 副作用: [
    // 一時directory内へテストCSVを作成する
    // ]
    // ```
    fn open(source: &str) -> CsvDocument {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, source).unwrap();
        CsvDocument::open(path).unwrap()
    }

    #[test]
    // ```text
    // 責務: [
    // header_lookup_rejects_missing_and_ambiguous_names: 重複・未検出headerの曖昧さを報告することを検証する
    // ]
    // 処理: [
    // 1: 重複名・未登録名・一意名それぞれの検索結果をassertする
    // ]
    // 引数: []
    // 戻り値: [(): 全assertion成功時に正常終了する]
    // ```
    fn header_lookup_rejects_missing_and_ambiguous_names() {
        let document = open("名前,名前,年齢\n田中,山田,20\n");

        assert_eq!(document.column_indices_by_header("名前"), vec![0, 1]);
        assert!(matches!(
            document.column_index_by_header("名前"),
            Err(ColumnError::AmbiguousHeader { .. })
        ));
        assert_eq!(
            document.column_index_by_header("住所"),
            Err(ColumnError::HeaderNotFound("住所".into()))
        );
        assert_eq!(document.column_index_by_header("年齢").unwrap(), 2);
    }

    #[test]
    // ```text
    // 責務: [
    // type_checks_do_not_mutate_canonical_csv_text: 列型判定結果とCSV原文の保持を検証する
    // ]
    // 処理: [
    // 1: 適合・不適合列のreportと判定後の元cell値をassertする
    // ]
    // 引数: []
    // 戻り値: [(): 全assertion成功時に正常終了する]
    // ```
    fn type_checks_do_not_mutate_canonical_csv_text() {
        let document = open("名前,年齢,有効\n田中,20,true\n山田,abc,false\n");

        let text = document
            .validate_column_type_by_header("名前", ColumnType::String)
            .unwrap();
        assert!(text.is_valid());
        assert_eq!(text.checked_cells(), 2);

        let integer = document
            .validate_column_type_by_header("年齢", ColumnType::Integer)
            .unwrap();
        assert!(!integer.is_valid());
        assert_eq!(integer.mismatches()[0].reference().to_string(), "B3");
        assert_eq!(integer.mismatches()[0].value(), "abc");
        assert_eq!(document.cell_a1("B3").unwrap(), Some("abc"));
    }

    #[test]
    // ```text
    // 責務: [
    // japanese_check_reports_matching_and_nonmatching_cells: 日本語文字の一致区分とA1位置を検証する
    // ]
    // 処理: [
    // 1: 混在値を検査し、件数・一致位置・不一致位置をassertする
    // ]
    // 引数: []
    // 戻り値: [(): 全assertion成功時に正常終了する]
    // ```
    fn japanese_check_reports_matching_and_nonmatching_cells() {
        let document = open("名前\n田中太郎\nAlice\n山田 Taro\n\"\"\n");
        let report = document.check_column_japanese_by_header("名前").unwrap();

        assert_eq!(report.checked_cells(), 4);
        assert_eq!(
            report
                .matches()
                .iter()
                .map(|cell| cell.reference().to_string())
                .collect::<Vec<_>>(),
            vec!["A2", "A4"]
        );
        assert_eq!(
            report
                .mismatches()
                .iter()
                .map(|cell| cell.reference().to_string())
                .collect::<Vec<_>>(),
            vec!["A3", "A5"]
        );
        assert!(!report.all_match());
    }

    #[test]
    // ```text
    // 責務: [
    // japanese_character_detection_covers_common_scripts: 日本語Unicode範囲と非対象文字の判定を検証する
    // ]
    // 処理: [
    // 1: 対象文字列はtrue、対象外文字列はfalseとなることをassertする
    // ]
    // 引数: []
    // 戻り値: [(): 全assertion成功時に正常終了する]
    // ```
    fn japanese_character_detection_covers_common_scripts() {
        for value in ["ひらがな", "カタカナ", "ﾊﾝｶｸ", "日本語", "山田 Taro", "々"]
        {
            assert!(contains_japanese(value), "{value}");
        }
        for value in ["Alice", "123", "", "hello-world"] {
            assert!(!contains_japanese(value), "{value}");
        }
    }
}
