use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};
use thiserror::Error;

use super::ColumnType;

const METADATA_VERSION: u64 = 1;

#[derive(Debug, Clone, Default)]
pub(super) struct ColumnMetadata {
    declarations: BTreeMap<String, ColumnType>,
}

impl ColumnMetadata {
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

    pub(super) fn set(&mut self, header: String, column_type: ColumnType) {
        self.declarations.insert(header, column_type);
    }

    pub(super) fn remove(&mut self, header: &str) -> bool {
        self.declarations.remove(header).is_some()
    }

    pub(super) fn get(&self, header: &str) -> Option<ColumnType> {
        self.declarations.get(header).copied()
    }

    pub(super) fn declarations(&self) -> impl Iterator<Item = (&str, ColumnType)> {
        self.declarations
            .iter()
            .map(|(header, column_type)| (header.as_str(), *column_type))
    }
}

pub(super) fn sidecar_path(csv_path: &Path) -> PathBuf {
    let mut value = csv_path.as_os_str().to_os_string();
    value.push(".rowly.json");
    PathBuf::from(value)
}

#[derive(Debug, Error)]
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
mod tests {
    use super::*;

    #[test]
    fn sidecar_path_appends_without_replacing_csv_extension() {
        assert_eq!(
            sidecar_path(Path::new("/tmp/data.csv")),
            PathBuf::from("/tmp/data.csv.rowly.json")
        );
    }
}
