use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};
use thiserror::Error;

use super::ColumnType;

// ```text
// 責務: [
// METADATA_VERSION: sidecar JSON schemaの現行version
// ]
// ```
const METADATA_VERSION: u64 = 1;

#[derive(Debug, Clone, Default)]
// ```text
// 責務: [
// ColumnMetadata: header名ごとの列型宣言を任意sidecarから管理する
// ]
// フィールド: [
// declarations: unique header nameから列型への対応
// ]
// 補足: [
// CSV本体がcanonical dataであり、metadataがなくても読み込み可能
// ]
// ```
pub(super) struct ColumnMetadata {
    declarations: BTreeMap<String, ColumnType>,
}

impl ColumnMetadata {
    // ```text
    // 責務: [
    // load: CSVに対応するJSON sidecarを読み、header別の列型宣言を復元する
    // ]
    // 処理: [
    // 1: sidecarがなければ空metadataを返す
    // 2: JSON versionとcolumns schemaを検証する
    // 3: 宣言型名をColumnTypeへ変換して格納する
    // ]
    // 引数: [
    // csv_path: sidecar名を決める元CSV path
    // ]
    // 戻り値: [
    // Result<ColumnMetadata, MetadataError>: 読み取った宣言、またはI/O・JSON・schema error
    // ]
    // ```
    pub(super) fn load(csv_path: &Path) -> Result<Self, MetadataError> {
        let path = sidecar_path(csv_path);
        if !path.exists() {
            return Ok(Self::default());
        }

        let bytes = fs::read(&path).map_err(|error| MetadataError::Read {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|error| MetadataError::Parse {
                path: path.display().to_string(),
                message: error.to_string(),
            })?;

        let version = value
            .get("version")
            .and_then(Value::as_u64)
            .ok_or_else(|| MetadataError::Schema("missing integer metadata version".into()))?;
        if version != METADATA_VERSION {
            return Err(MetadataError::Schema(format!(
                "unsupported metadata version {version}"
            )));
        }

        let columns = value
            .get("columns")
            .and_then(Value::as_object)
            .ok_or_else(|| MetadataError::Schema("missing columns object".into()))?;

        let mut declarations = BTreeMap::new();
        for (header, declaration) in columns {
            let type_name = declaration
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    MetadataError::Schema(format!(
                        "column metadata for `{header}` does not contain a string type"
                    ))
                })?;
            let column_type = ColumnType::from_metadata_str(type_name).ok_or_else(|| {
                MetadataError::Schema(format!(
                    "column metadata for `{header}` has unsupported type `{type_name}`"
                ))
            })?;
            declarations.insert(header.clone(), column_type);
        }

        Ok(Self { declarations })
    }

    // ```text
    // 責務: [
    // save: 列型宣言をCSVに対応するJSON sidecarへ保存する
    // ]
    // 処理: [
    // 1: versionとheader別宣言を含むJSONを整形する
    // 2: sidecar pathへ内容を書き込む
    // ]
    // 引数: [
    // self: 保存する列型宣言
    // csv_path: sidecar名を決める元CSV path
    // ]
    // 戻り値: [
    // Result<(), MetadataError>: 書き込み結果、またはJSON生成・file書込error
    // ]
    // 副作用: [
    // CSV本体とは別のsidecar fileを作成または上書きする
    // ]
    // ```
    pub(super) fn save(&self, csv_path: &Path) -> Result<(), MetadataError> {
        let path = sidecar_path(csv_path);
        let columns = self
            .declarations
            .iter()
            .map(|(header, column_type)| {
                (
                    header.clone(),
                    json!({
                        "type": column_type.as_metadata_str(),
                    }),
                )
            })
            .collect::<serde_json::Map<String, Value>>();

        let content = serde_json::to_vec_pretty(&json!({
            "version": METADATA_VERSION,
            "columns": columns,
        }))
        .map_err(|error| MetadataError::Schema(error.to_string()))?;

        fs::write(&path, content).map_err(|error| MetadataError::Write {
            path: path.display().to_string(),
            message: error.to_string(),
        })
    }

    // ```text
    // 責務: [
    // set: header名に列型宣言を登録または置換する
    // ]
    // 処理: [
    // 1: header keyにcolumn typeをinsertし、既存宣言があれば置換する
    // ]
    // 引数: [
    // self: 変更対象metadata
    // header: 宣言を結び付ける列名
    // column_type: 登録する列型
    // ]
    // 戻り値: [unit]
    // ```
    pub(super) fn set(&mut self, header: String, column_type: ColumnType) {
        self.declarations.insert(header, column_type);
    }

    // ```text
    // 責務: [
    // remove: header名の列型宣言を削除する
    // ]
    // 処理: [
    // 1: header keyをmapから削除して存在有無を返す
    // ]
    // 引数: [
    // self: 変更対象metadata
    // header: 削除する宣言の列名
    // ]
    // 戻り値: [
    // bool: 宣言が存在して削除された場合true
    // ]
    // ```
    pub(super) fn remove(&mut self, header: &str) -> bool {
        self.declarations.remove(header).is_some()
    }

    // ```text
    // 責務: [
    // get: header名に対応する宣言型を取得する
    // ]
    // 処理: [
    // 1: mapから型を検索し、Copyして返す
    // ]
    // 引数: [
    // self: 検索対象metadata
    // header: 検索する列名
    // ]
    // 戻り値: [
    // Option<ColumnType>: 登録済み型、または未宣言時None
    // ]
    // ```
    pub(super) fn get(&self, header: &str) -> Option<ColumnType> {
        self.declarations.get(header).copied()
    }

    // ```text
    // 責務: [
    // declarations: header名と列型宣言の借用iteratorを返す
    // ]
    // 処理: [
    // 1: map entryをheader borrowとCopyした型のpairへ写す
    // ]
    // 引数: [
    // self: 列挙対象metadata
    // ]
    // 戻り値: [
    // impl Iterator<Item = (&str, ColumnType)>: 宣言順のheader/type pairs
    // ]
    // ```
    pub(super) fn declarations(&self) -> impl Iterator<Item = (&str, ColumnType)> {
        self.declarations
            .iter()
            .map(|(header, column_type)| (header.as_str(), *column_type))
    }
}

// ```text
// 責務: [
// sidecar_path: CSV path末尾へRowly metadata suffixを追加したpathを作る
// ]
// 処理: [
// 1: 元pathを保持したままsuffixをos string末尾へ追加する
// ]
// 引数: [
// csv_path: 元CSVのpath
// ]
// 戻り値: [
// PathBuf: extensionを置換せずsuffixを追加したsidecar path
// ]
// ```
pub(super) fn sidecar_path(csv_path: &Path) -> PathBuf {
    let mut value = csv_path.as_os_str().to_os_string();
    value.push(".rowly.json");
    PathBuf::from(value)
}

#[derive(Debug, Error)]
// ```text
// 責務: [
// MetadataError: sidecar読み込み・書き込み・schema検証の失敗を表す
// ]
// 補足: [
// Read: sidecar bytesを読めない
// Parse: sidecar bytesがJSONとして不正
// Schema: versionまたは列宣言形式が未対応・不正
// Write: sidecar内容を書き込めない
// ]
// ```
pub(super) enum MetadataError {
    #[error("failed to read Rowly metadata `{path}`: {message}")]
    Read { path: String, message: String },

    #[error("failed to parse Rowly metadata `{path}`: {message}")]
    Parse { path: String, message: String },

    #[error("invalid Rowly metadata schema: {0}")]
    Schema(String),

    #[error("failed to write Rowly metadata `{path}`: {message}")]
    Write { path: String, message: String },
}

#[cfg(test)]
// ```text
// 責務: [
// tests: sidecar pathがCSV extensionを保持してsuffixを追加することを検証する
// ]
// ```
mod tests {
    use super::*;

    #[test]
    // ```text
    // 責務: [
    // sidecar_path_appends_without_replacing_csv_extension: sidecar命名規則を検証する
    // ]
    // 処理: [
    // 1: csv pathへsuffixを加えた結果が期待pathと一致することをassertする
    // ]
    // 引数: []
    // 戻り値: [(): 全assertion成功時に正常終了する]
    // ```
    fn sidecar_path_appends_without_replacing_csv_extension() {
        assert_eq!(
            sidecar_path(Path::new("/tmp/data.csv")),
            PathBuf::from("/tmp/data.csv.rowly.json")
        );
    }
}
