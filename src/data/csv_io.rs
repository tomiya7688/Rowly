use std::{
    collections::hash_map::DefaultHasher,
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::{self, BufWriter},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use csv::{ReaderBuilder, Terminator, WriterBuilder};
use thiserror::Error;

use super::{SourceEncoding, Table, encoding::decode_to_utf8};

// {
//   責務: [
//     LoadedCsv: 読み込んだ表、元encoding、外部変更検出用fingerprintをまとめる
//   ]
//   フィールド: [
//     table: CSV recordを保持する表
//     encoding: 入力CSVの元文字コード
//     fingerprint: 読込時bytesのcontent fingerprint
//   ]
// }
pub(crate) struct LoadedCsv {
    pub(crate) table: Table,
    pub(crate) encoding: SourceEncoding,
    pub(crate) fingerprint: ContentFingerprint,
}

// {
//   責務: [
//     ContentFingerprint: file contentの比較に使う固定長fingerprintを保持する
//   ]
//   フィールド: [
//     length: 元bytesの長さ
//     first: 1つ目のseedによるhash値
//     second: 別seedによるhash値
//   ]
// }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContentFingerprint {
    length: u64,
    first: u64,
    second: u64,
}

impl ContentFingerprint {
    // {
    //   責務: [
    //     from_bytes: bytesから長さと独立seedのhashを持つfingerprintを作る
    //   ]
    //   引数: [
    //     bytes: 比較対象のfile content
    //   ]
    //   戻り値: [
    //     Self: file contentの比較用fingerprint
    //   ]
    // }
    fn from_bytes(bytes: &[u8]) -> Self {
        let mut first = DefaultHasher::new();
        first.write_u64(0x726f_776c_792d_3031);
        bytes.hash(&mut first);
        let mut second = DefaultHasher::new();
        second.write_u64(0x726f_776c_792d_3032);
        bytes.hash(&mut second);
        Self {
            length: bytes.len() as u64,
            first: first.finish(),
            second: second.finish(),
        }
    }
}

// {
//   責務: [
//     fingerprint_file: fileを読み、その現在のcontent fingerprintを返す
//   ]
//   引数: [
//     path: fingerprintを取得するfile path
//   ]
//   戻り値: [
//     ContentFingerprint: file contentの比較用fingerprint
//     io::Error: fileを読み込めない理由
//   ]
// }
pub(crate) fn fingerprint_file(path: &Path) -> io::Result<ContentFingerprint> {
    fs::read(path).map(|bytes| ContentFingerprint::from_bytes(&bytes))
}

// {
//   責務: [
//     read_csv: fileを読み、文字コードとCSV構造を保持したLoadedCsvを作る
//   ]
//   引数: [
//     path: 読み込むCSV file path
//   ]
//   戻り値: [
//     LoadedCsv: decode済みtable、元encoding、読込時fingerprint
//     CsvIoError: file、encoding、CSV parseの失敗理由
//   ]
// }
pub(crate) fn read_csv(path: &Path) -> Result<LoadedCsv, CsvIoError> {
    let bytes = fs::read(path)?;
    let decoded = decode_to_utf8(&bytes)?;

    let mut reader = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(decoded.text.as_bytes());

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        rows.push(record.iter().map(str::to_owned).collect());
    }

    Ok(LoadedCsv {
        table: Table::new(rows),
        encoding: decoded.encoding,
        fingerprint: ContentFingerprint::from_bytes(&bytes),
    })
}

// {
//   責務: [
//     write_csv_utf8: TableをUTF-8 CSVとして書き込み、保存後fingerprintを返す
//   ]
//   引数: [
//     path: 書込先CSV file path
//     table: CSVへ保存する表
//   ]
//   戻り値: [
//     ContentFingerprint: 保存されたfileのfingerprint
//     CsvIoError: CSVを書き込めない理由
//   ]
// }
pub(crate) fn write_csv_utf8(path: &Path, table: &Table) -> Result<ContentFingerprint, CsvIoError> {
    write_csv_utf8_if_unchanged(path, table, None)
}

// {
//   責務: [
//     write_csv_utf8_if_unchanged: CSVを一時fileへ完全に書き、必要なら旧content確認後に置換する
//   ]
//   処理: [
//     1: UTF-8・LFのCSVを同じdirectoryの一時fileへ書く
//     2: expectedがある場合は保存先の現fingerprintと比較する
//     3: 一致するときだけ保存先を置換し、書込・比較不一致・置換失敗では一時fileを除去する
//   ]
//   引数: [
//     path: 保存先CSV file path
//     table: 書き込む表
//     expected: 設定時に保存先が一致すべきfingerprint
//   ]
//   戻り値: [
//     ContentFingerprint: 新しく保存したcontentのfingerprint
//     CsvIoError: I/O、CSV書込、外部変更の理由
//   ]
//   副作用: [
//     一時fileを作成し、成功時に保存先fileを置換する
//   ]
// }
pub(crate) fn write_csv_utf8_if_unchanged(
    path: &Path,
    table: &Table,
    expected: Option<ContentFingerprint>,
) -> Result<ContentFingerprint, CsvIoError> {
    let (temporary, file) = create_temporary_csv(path)?;
    let write_result = (|| {
        let buffer = BufWriter::new(file);
        let mut writer = WriterBuilder::new()
            .terminator(Terminator::Any(b'\n'))
            .from_writer(buffer);
        for row in table.rows() {
            writer.write_record(row)?;
        }
        writer.flush()?;
        writer.get_ref().get_ref().sync_all()?;
        Ok::<(), CsvIoError>(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }

    let fingerprint = match fingerprint_file(&temporary) {
        Ok(fingerprint) => fingerprint,
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(CsvIoError::Io(error));
        }
    };
    if let Some(expected) = expected {
        let current = match fingerprint_file(path) {
            Ok(current) => current,
            Err(_) => {
                let _ = fs::remove_file(&temporary);
                return Err(CsvIoError::ExternalModification);
            }
        };
        if current != expected {
            let _ = fs::remove_file(&temporary);
            return Err(CsvIoError::ExternalModification);
        }
    }
    if let Err(error) = replace_file(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(CsvIoError::Io(error));
    }
    Ok(fingerprint)
}

// {
//   責務: [
//     create_temporary_csv: 保存先と同じdirectoryに衝突しない一時CSV fileを作る
//   ]
//   引数: [
//     path: 保存先CSV file path
//   ]
//   戻り値: [
//     (PathBuf, File): 作成した一時fileのpathと書込handle
//     CsvIoError: pathが無効、または一時fileを作れない理由
//   ]
//   副作用: [
//     create_newで一時fileを排他的に作成する
//   ]
// }
fn create_temporary_csv(path: &Path) -> Result<(PathBuf, File), CsvIoError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or_else(|| {
        CsvIoError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "CSV path has no file name",
        ))
    })?;
    for attempt in 0..16u32 {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut temporary_name = name.to_os_string();
        temporary_name.push(format!(".{}.{}.{}.tmp", std::process::id(), stamp, attempt));
        let temporary = parent.join(temporary_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(CsvIoError::Io(error)),
        }
    }
    Err(CsvIoError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a temporary CSV file",
    )))
}

// {
//   責務: [
//     replace_file: Windowsのreplace-capable file moveで保存先を置換する
//   ]
//   引数: [
//     source: 完成済み一時file
//     destination: 置換する保存先file
//   ]
//   戻り値: [
//     (): file置換の成功
//     io::Error: OSによる置換失敗の理由
//   ]
//   副作用: [
//     destinationをsourceのfileへ置換する
//   ]
// }
#[cfg(windows)]
pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        // {
        //   責務: [
        //     MoveFileExW: Windows Kernel32のfile move APIを呼び出す
        //   ]
        //   引数: [
        //     existing: NUL終端UTF-16のsource path
        //     new: NUL終端UTF-16のdestination path
        //     flags: replaceとwrite-through動作の指定
        //   ]
        //   戻り値: [
        //     i32: 成功時は非zero、失敗時はzero
        //   ]
        // }
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: both pointers reference NUL-terminated UTF-16 buffers for this call.
    let result = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0x1 | 0x8) };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

// {
//   責務: [
//     replace_file: filesystem renameで一時fileを保存先へ置換する
//   ]
//   引数: [
//     source: 完成済み一時file
//     destination: 置換する保存先file
//   ]
//   戻り値: [
//     (): renameの成功
//     io::Error: filesystemによる置換失敗の理由
//   ]
//   副作用: [
//     destinationをsourceのfileへ置換する
//   ]
// }
#[cfg(not(windows))]
pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

// {
//   責務: [
//     CsvIoError: CSV file処理で発生するI/O、encoding、parse、競合errorを表す
//   ]
//   フィールド: [
//     Io: filesystemまたはwriterのI/O error
//     Encoding: CSV本文を許可されたencodingへ変換できないerror
//     Csv: CSV recordの解析または書込error
//     ExternalModification: open後に保存先fileが変更されたことを示すerror
//   ]
// }
#[derive(Debug, Error)]
pub(crate) enum CsvIoError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("encoding error: {0}")]
    Encoding(#[from] super::encoding::DecodeError),

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    #[error("CSV changed on disk after it was opened")]
    ExternalModification,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    // {
    //   責務: [
    //     reads_empty_values_without_rejecting_the_csv: 空fieldを空文字として読み取る
    //   ]
    //   処理: [
    //     1: 行末と途中の空fieldを含むCSVを読む
    //     2: 空fieldと隣接する値が保持されることを確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn reads_empty_values_without_rejecting_the_csv() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("empty.csv");
        fs::write(&path, "name,age,note\n田中,20,\n佐藤,,確認中\n").unwrap();

        let loaded = read_csv(&path).unwrap();

        assert_eq!(loaded.table.cell(1, 2), Some(""));
        assert_eq!(loaded.table.cell(2, 1), Some(""));
        assert_eq!(loaded.table.cell(2, 2), Some("確認中"));
    }

    // {
    //   責務: [
    //     removes_temporary_file_when_source_fingerprint_fails: 保存元のfingerprint取得失敗時に一時CSVを残さない
    //   ]
    //   処理: [
    //     1: 保存元のfingerprintを取得してから保存元fileを削除する
    //     2: fingerprint付き保存がExternalModificationとなることを確認する
    //     3: directoryに一時CSVも保存先fileも残らないことを確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn removes_temporary_file_when_source_fingerprint_fails() {
        let directory = tempdir().unwrap();
        let source_path = directory.path().join("source.csv");
        fs::write(&source_path, "name\nsource\n").unwrap();
        let expected_fingerprint = fingerprint_file(&source_path).unwrap();
        fs::remove_file(&source_path).unwrap();
        let table = Table::new(vec![vec!["replacement".to_owned()]]);

        let error = write_csv_utf8_if_unchanged(&source_path, &table, Some(expected_fingerprint))
            .unwrap_err();

        assert!(matches!(error, CsvIoError::ExternalModification));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    // {
    //   責務: [
    //     reads_standard_csv_quoting_and_embedded_newlines: 標準引用符表現をrecordとfield値へ復元する
    //   ]
    //   処理: [
    //     1: comma、double quote、embedded newlineを含むCSVを読む
    //     2: record数とdecode後のfield値を確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn reads_standard_csv_quoting_and_embedded_newlines() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("quoted.csv");
        fs::write(
            &path,
            "name,note\r\n田中,\"東京,大阪\"\r\n佐藤,\"彼は\"\"OK\"\"と言った\"\r\n鈴木,\"1行目\n2行目\"\r\n",
        )
        .unwrap();

        let loaded = read_csv(&path).unwrap();

        assert_eq!(loaded.table.row_count(), 4);
        assert_eq!(loaded.table.cell(1, 1), Some("東京,大阪"));
        assert_eq!(loaded.table.cell(2, 1), Some("彼は\"OK\"と言った"));
        assert_eq!(loaded.table.cell(3, 1), Some("1行目\n2行目"));
    }

    // {
    //   責務: [
    //     accepts_lf_and_crlf_and_writes_utf8_lf: LFとCRLFを読み、保存時にUTF-8 LFへ正規化する
    //   ]
    //   処理: [
    //     1: LFとCRLFのCSVが同じtableになることを確認する
    //     2: 書込結果のencodingと改行を確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn accepts_lf_and_crlf_and_writes_utf8_lf() {
        let directory = tempdir().unwrap();
        let lf_path = directory.path().join("lf.csv");
        let crlf_path = directory.path().join("crlf.csv");
        let output_path = directory.path().join("output.csv");
        fs::write(&lf_path, "a,b\n1,2\n").unwrap();
        fs::write(&crlf_path, "a,b\r\n1,2\r\n").unwrap();

        let lf = read_csv(&lf_path).unwrap();
        let crlf = read_csv(&crlf_path).unwrap();
        assert_eq!(lf.table, crlf.table);

        write_csv_utf8(&output_path, &crlf.table).unwrap();
        let bytes = fs::read(&output_path).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();

        assert_eq!(text, "a,b\n1,2\n");
        assert!(!text.contains("\r\n"));
    }

    // {
    //   責務: [
    //     round_trip_preserves_special_field_values: 空値や引用が必要なfieldを往復後も保持する
    //   ]
    //   処理: [
    //     1: 特殊fieldを含むCSVを読み書きする
    //     2: 再読込後のtableと各field値を比較する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn round_trip_preserves_special_field_values() {
        let directory = tempdir().unwrap();
        let source_path = directory.path().join("special.csv");
        let output_path = directory.path().join("special-output.csv");
        fs::write(
            &source_path,
            "value\n\"\"\n\"a,b\"\n\"a\"\"b\"\n\"line1\nline2\"\n",
        )
        .unwrap();

        let loaded = read_csv(&source_path).unwrap();
        write_csv_utf8(&output_path, &loaded.table).unwrap();
        let reopened = read_csv(&output_path).unwrap();

        assert_eq!(loaded.table, reopened.table);
        assert_eq!(reopened.table.cell(1, 0), Some(""));
        assert_eq!(reopened.table.cell(2, 0), Some("a,b"));
        assert_eq!(reopened.table.cell(3, 0), Some("a\"b"));
        assert_eq!(reopened.table.cell(4, 0), Some("line1\nline2"));
    }

    // {
    //   責務: [
    //     round_trip_preserves_records_and_values: CSVのrecordとfield値を読み書き後も保持する
    //   ]
    //   処理: [
    //     1: 通常fieldを含むCSVを読み書きする
    //     2: 再読込tableと元encodingを確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn round_trip_preserves_records_and_values() {
        let directory = tempdir().unwrap();
        let source_path = directory.path().join("source.csv");
        let output_path = directory.path().join("output.csv");
        fs::write(
            &source_path,
            "name,note\nAlice,\"hello, world\"\nBob,plain\n",
        )
        .unwrap();

        let loaded = read_csv(&source_path).unwrap();
        write_csv_utf8(&output_path, &loaded.table).unwrap();
        let reopened = read_csv(&output_path).unwrap();

        assert_eq!(loaded.table, reopened.table);
        assert_eq!(reopened.encoding, SourceEncoding::Utf8);
    }
}
