use std::fmt;

use chardetng::EncodingDetector;
use encoding_rs::SHIFT_JIS;
use thiserror::Error;

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceEncoding {
    Utf8,
    ShiftJis,
}

impl fmt::Display for SourceEncoding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utf8 => formatter.write_str("UTF-8"),
            Self::ShiftJis => formatter.write_str("Shift_JIS"),
        }
    }
}

pub(crate) struct DecodedText {
    pub(crate) text: String,
    pub(crate) encoding: SourceEncoding,
}

pub(crate) fn decode_to_utf8(bytes: &[u8]) -> Result<DecodedText, DecodeError> {
    let utf8_candidate = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    if let Ok(text) = std::str::from_utf8(utf8_candidate) {
        return Ok(DecodedText {
            text: text.to_owned(),
            encoding: SourceEncoding::Utf8,
        });
    }

    let mut detector = EncodingDetector::new();
    detector.feed(bytes, true);
    let detected = detector.guess(None, true);

    if detected.name() != SHIFT_JIS.name() {
        return Err(DecodeError::UnsupportedEncoding(detected.name()));
    }

    let (decoded, had_errors) = SHIFT_JIS.decode_without_bom_handling(bytes);
    if had_errors {
        return Err(DecodeError::InvalidShiftJis);
    }

    Ok(DecodedText {
        text: decoded.into_owned(),
        encoding: SourceEncoding::ShiftJis,
    })
}

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum DecodeError {
    #[error("unsupported text encoding detected: {0}")]
    UnsupportedEncoding(&'static str),

    #[error("input was detected as Shift_JIS but contains invalid byte sequences")]
    InvalidShiftJis,
}

#[cfg(test)]
mod tests {
    use encoding_rs::SHIFT_JIS;

    use super::*;

    #[test]
    fn strips_utf8_bom() {
        let decoded = decode_to_utf8(b"\xEF\xBB\xBFname,value\n").unwrap();

        assert_eq!(decoded.encoding, SourceEncoding::Utf8);
        assert_eq!(decoded.text, "name,value\n");
    }

    #[test]
    fn decodes_shift_jis_to_utf8() {
        let source = "部署,名前\n営業部,田中太郎\n営業部,佐藤花子\n開発部,山田一郎\n";
        let (encoded, _, had_errors) = SHIFT_JIS.encode(source);
        assert!(!had_errors);

        let decoded = decode_to_utf8(encoded.as_ref()).unwrap();

        assert_eq!(decoded.encoding, SourceEncoding::ShiftJis);
        assert_eq!(decoded.text, source);
    }
}
