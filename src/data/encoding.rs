use std::fmt;

use chardetng::EncodingDetector;
use encoding_rs::SHIFT_JIS;
use thiserror::Error;

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

// {
//   責務: [
//     SourceEncoding: 読み込んだCSVの元文字コードを表す
//   ]
//   フィールド: [
//     Utf8: UTF-8として読み込まれた本文
//     ShiftJis: Shift_JISからUTF-8へ変換された本文
//   ]
// }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceEncoding {
    Utf8,
    ShiftJis,
}

impl fmt::Display for SourceEncoding {
    // {
    //   責務: [
    //     fmt: encodingの識別名を標準的な文字列として書式化する
    //   ]
    //   処理: [
    //     1: variantに対応するUTF-8またはShift_JIS名をformatterへ書き込む
    //   ]
    //   引数: [
    //     formatter: encoding名を書き込む出力先
    //   ]
    //   戻り値: [
    //     fmt::Result: 書式化の成否
    //   ]
    // }
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utf8 => formatter.write_str("UTF-8"),
            Self::ShiftJis => formatter.write_str("Shift_JIS"),
        }
    }
}

// {
//   責務: [
//     DecodedText: UTF-8へ変換した本文と元encodingをまとめる
//   ]
//   フィールド: [
//     text: CSV parserへ渡すUTF-8本文
//     encoding: 読込元の文字コード
//   ]
// }
pub(crate) struct DecodedText {
    pub(crate) text: String,
    pub(crate) encoding: SourceEncoding,
}

// {
//   責務: [
//     decode_to_utf8: bytesを許可された文字コードからUTF-8本文へ変換する
//   ]
//   処理: [
//     1: UTF-8 BOMを除いてUTF-8として検証する
//     2: UTF-8でなければ文字コードを判定しShift_JISだけを変換する
//     3: 未対応encodingまたは不正なShift_JIS bytesをerrorにする
//   ]
//   引数: [
//     bytes: CSVファイルから読み込んだ元bytes
//   ]
//   戻り値: [
//     DecodedText: UTF-8本文と元encoding
//     DecodeError: 未対応encodingまたは変換失敗の理由
//   ]
// }
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

// {
//   責務: [
//     DecodeError: CSV本文を許可された文字コードへ変換できない理由を表す
//   ]
//   フィールド: [
//     UnsupportedEncoding: 検出されたが対応していないencoding名
//     InvalidShiftJis: Shift_JISとして不正なbyte列
//   ]
// }
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

    // {
    //   責務: [
    //     strips_utf8_bom: UTF-8 BOMを本文へ残さず読み込めることを確認する
    //   ]
    //   処理: [
    //     1: BOM付きUTF-8 bytesを変換する
    //     2: encodingと本文からBOMが除かれたことを確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn strips_utf8_bom() {
        let decoded = decode_to_utf8(b"\xEF\xBB\xBFname,value\n").unwrap();

        assert_eq!(decoded.encoding, SourceEncoding::Utf8);
        assert_eq!(decoded.text, "name,value\n");
    }

    // {
    //   責務: [
    //     decodes_shift_jis_to_utf8: Shift_JIS CSV本文をUTF-8へ変換できることを確認する
    //   ]
    //   処理: [
    //     1: 日本語CSV本文をShift_JISへencodeする
    //     2: decoderが元本文とencodingを復元することを確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
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
