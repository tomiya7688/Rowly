// {
//   責務: [
//     csv_io: CSVファイルの読込、書込、競合検出を提供する
//   ]
// }
mod csv_io;
// {
//   責務: [
//     encoding: 入力文字コードを検出し内部UTF-8へ変換する
//   ]
// }
mod encoding;
// {
//   責務: [
//     table: 可変長行を含むCSV表データの基本操作を提供する
//   ]
// }
mod table;

pub(crate) use csv_io::{
    ContentFingerprint, CsvIoError, LoadedCsv, fingerprint_file, read_csv, replace_file,
    write_csv_utf8, write_csv_utf8_if_unchanged,
};
pub use encoding::SourceEncoding;
pub(crate) use table::Table;
