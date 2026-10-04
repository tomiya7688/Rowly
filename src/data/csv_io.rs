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

pub(crate) struct LoadedCsv {
    pub(crate) table: Table,
    pub(crate) encoding: SourceEncoding,
    pub(crate) fingerprint: ContentFingerprint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContentFingerprint {
    length: u64,
    first: u64,
    second: u64,
}

impl ContentFingerprint {
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

pub(crate) fn fingerprint_file(path: &Path) -> io::Result<ContentFingerprint> {
    fs::read(path).map(|bytes| ContentFingerprint::from_bytes(&bytes))
}

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

pub(crate) fn write_csv_utf8(path: &Path, table: &Table) -> Result<ContentFingerprint, CsvIoError> {
    write_csv_utf8_if_unchanged(path, table, None)
}

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
        let current = fingerprint_file(path).map_err(|_| CsvIoError::ExternalModification)?;
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

#[cfg(windows)]
pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "Kernel32")]
    unsafe extern "system" {
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

#[cfg(not(windows))]
pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

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
