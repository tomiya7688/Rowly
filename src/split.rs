//! Dynamic view rules that partition logical table rows by column values.

use std::collections::HashMap;

use serde_json::{Value, json};
use thiserror::Error;

use crate::logical_table::LogicalTable;

const SPLIT_FORMAT: &str = "rowly-split-definition";
const SPLIT_VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitDefinition {
    id: String,
    key_columns: Vec<usize>,
    display_names: HashMap<Vec<SplitKeyValue>, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SplitKeyValue {
    Value(String),
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SplitSheetIdentity {
    pub split_definition_id: String,
    pub key_columns: Vec<usize>,
    pub key_values: Vec<SplitKeyValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitSheet {
    pub identity: SplitSheetIdentity,
    /// User-facing label kept separate from the stable split identity.
    pub display_name: String,
    /// Current indices into `LogicalTable::rows`, recalculated on every evaluation.
    pub row_indices: Vec<usize>,
}

impl SplitDefinition {
    pub fn new(id: impl Into<String>, key_columns: Vec<usize>) -> Result<Self, SplitError> {
        let definition = Self {
            id: id.into(),
            key_columns,
            display_names: HashMap::new(),
        };
        definition.validate()?;
        Ok(definition)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn key_columns(&self) -> &[usize] {
        &self.key_columns
    }

    pub fn set_display_name(
        &mut self,
        key_values: Vec<SplitKeyValue>,
        display_name: impl Into<String>,
    ) -> Result<(), SplitError> {
        if key_values.len() != self.key_columns.len() {
            return Err(SplitError::KeyArity {
                expected: self.key_columns.len(),
                actual: key_values.len(),
            });
        }
        let display_name = display_name.into();
        if display_name.trim().is_empty() {
            return Err(SplitError::EmptyDisplayName);
        }
        self.display_names.insert(key_values, display_name);
        Ok(())
    }

    /// Reevaluate membership from current cell values; row indexes are output
    /// references only and are never saved as the split definition.
    pub fn evaluate(&self, table: &LogicalTable) -> Result<Vec<SplitSheet>, SplitError> {
        self.validate()?;
        for column in &self.key_columns {
            if *column >= table.schema.len() {
                return Err(SplitError::ColumnOutOfRange {
                    column: *column,
                    width: table.schema.len(),
                });
            }
        }

        let mut sheets = Vec::<SplitSheet>::new();
        let mut sheet_by_key = HashMap::<Vec<SplitKeyValue>, usize>::new();
        for (row_index, row) in table.rows.iter().enumerate() {
            let key_values = self
                .key_columns
                .iter()
                .map(|column| match row.values.get(*column) {
                    Some(value) => SplitKeyValue::Value(value.clone()),
                    None => SplitKeyValue::Missing,
                })
                .collect::<Vec<_>>();
            let sheet_index = *sheet_by_key.entry(key_values.clone()).or_insert_with(|| {
                let identity = SplitSheetIdentity {
                    split_definition_id: self.id.clone(),
                    key_columns: self.key_columns.clone(),
                    key_values: key_values.clone(),
                };
                let display_name = self
                    .display_names
                    .get(&key_values)
                    .cloned()
                    .unwrap_or_else(|| default_display_name(&self.id, &identity.key_values));
                let index = sheets.len();
                sheets.push(SplitSheet {
                    identity,
                    display_name,
                    row_indices: Vec::new(),
                });
                index
            });
            sheets[sheet_index].row_indices.push(row_index);
        }
        Ok(sheets)
    }

    pub fn to_json(&self) -> Value {
        let mut display_names = self
            .display_names
            .iter()
            .map(|(key_values, name)| {
                json!({
                    "key_values": key_values.iter().map(key_to_json).collect::<Vec<_>>(),
                    "display_name": name,
                })
            })
            .collect::<Vec<_>>();
        display_names.sort_by(|left, right| {
            left["key_values"]
                .to_string()
                .cmp(&right["key_values"].to_string())
        });
        json!({
            "format": SPLIT_FORMAT,
            "version": SPLIT_VERSION,
            "id": self.id,
            "key_columns": self.key_columns,
            "display_names": display_names,
        })
    }

    pub fn from_json(value: Value) -> Result<Self, SplitError> {
        let object = value
            .as_object()
            .ok_or(SplitError::InvalidDefinition("expected an object"))?;
        if object.get("format").and_then(Value::as_str) != Some(SPLIT_FORMAT) {
            return Err(SplitError::InvalidDefinition("unsupported format"));
        }
        if object.get("version").and_then(Value::as_u64) != Some(SPLIT_VERSION) {
            return Err(SplitError::InvalidDefinition("unsupported version"));
        }
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .ok_or(SplitError::InvalidDefinition("missing id"))?
            .to_owned();
        let key_columns = object
            .get("key_columns")
            .and_then(Value::as_array)
            .ok_or(SplitError::InvalidDefinition("missing key_columns"))?
            .iter()
            .map(|column| {
                column
                    .as_u64()
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or(SplitError::InvalidDefinition(
                        "key columns must be non-negative integers",
                    ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut definition = Self::new(id, key_columns)?;
        let display_names = object
            .get("display_names")
            .and_then(Value::as_array)
            .ok_or(SplitError::InvalidDefinition("missing display_names"))?;
        for entry in display_names {
            let entry = entry.as_object().ok_or(SplitError::InvalidDefinition(
                "display name entries must be objects",
            ))?;
            let key_values = entry
                .get("key_values")
                .and_then(Value::as_array)
                .ok_or(SplitError::InvalidDefinition(
                    "missing display name key_values",
                ))?
                .iter()
                .map(key_from_json)
                .collect::<Result<Vec<_>, _>>()?;
            let name = entry
                .get("display_name")
                .and_then(Value::as_str)
                .ok_or(SplitError::InvalidDefinition("missing display_name"))?;
            definition.set_display_name(key_values, name)?;
        }
        Ok(definition)
    }

    fn validate(&self) -> Result<(), SplitError> {
        if self.id.trim().is_empty() {
            return Err(SplitError::InvalidDefinition("id must not be empty"));
        }
        if self.key_columns.is_empty() {
            return Err(SplitError::InvalidDefinition(
                "at least one key column is required",
            ));
        }
        let mut columns = self.key_columns.clone();
        columns.sort_unstable();
        columns.dedup();
        if columns.len() != self.key_columns.len() {
            return Err(SplitError::InvalidDefinition("key columns must be unique"));
        }
        Ok(())
    }
}

fn key_to_json(value: &SplitKeyValue) -> Value {
    match value {
        SplitKeyValue::Value(value) => Value::String(value.clone()),
        SplitKeyValue::Missing => Value::Null,
    }
}

fn key_from_json(value: &Value) -> Result<SplitKeyValue, SplitError> {
    match value {
        Value::String(value) => Ok(SplitKeyValue::Value(value.clone())),
        Value::Null => Ok(SplitKeyValue::Missing),
        _ => Err(SplitError::InvalidDefinition(
            "key values must be strings or null for missing values",
        )),
    }
}

fn default_display_name(id: &str, values: &[SplitKeyValue]) -> String {
    let values = values
        .iter()
        .map(|value| match value {
            SplitKeyValue::Value(value) if value.is_empty() => "(empty)".to_owned(),
            SplitKeyValue::Value(value) => value.clone(),
            SplitKeyValue::Missing => "(missing)".to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" / ");
    format!("{id}: {values}")
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SplitError {
    #[error("invalid split definition: {0}")]
    InvalidDefinition(&'static str),
    #[error("split key column {column} is outside table width {width}")]
    ColumnOutOfRange { column: usize, width: usize },
    #[error("split key has {actual} values, expected {expected}")]
    KeyArity { expected: usize, actual: usize },
    #[error("display name must not be empty")]
    EmptyDisplayName,
}
