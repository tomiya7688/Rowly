use std::{
    fs::{self, File},
    io::{self, BufWriter},
    path::Path,
};

use csv::{ReaderBuilder, Terminator, WriterBuilder};
use thiserror::Error;

use super::{SourceEncoding, Table, encoding::decode_to_utf8};

pub(crate) struct LoadedCsv {
    pub(crate) table: Table,
    pub(crate) encoding: SourceEncoding,
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
    })
}

pub(crate) fn write_csv_utf8(path: &Path, table: &Table) -> Result<(), CsvIoError> {
    let file = File::create(path)?;
    let buffer = BufWriter::new(file);
    let mut writer = WriterBuilder::new()
        .terminator(Terminator::Any(b'\n'))
        .from_writer(buffer);

    for row in table.rows() {
        writer.write_record(row)?;
    }

    writer.flush()?;
    Ok(())
}

#[derive(Debug, Error)]
pub(crate) enum CsvIoError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("encoding error: {0}")]
    Encoding(#[from] super::encoding::DecodeError),

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

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
