//! ```text
//! 責務: [
//! process: CSV documentを扱うA1/range addressing、列検証、編集履歴、validation、metadata機能を公開する
//! ]
//! 処理: [
//! 1: data層の文字列tableを利用する操作型と結果型を再exportする
//! 2: GUI featureが有効な場合はCSV変更watcherを内部公開する
//! ]
//! 補足: [
//! CSVの読書きやcanonical table modelはcrate::dataが担当する
//! ]
//! ```
mod address;
mod column;
mod document;
mod history;
mod metadata;
mod validation;
#[cfg(feature = "gui")]
mod watcher;

pub use crate::data::SourceEncoding;
pub use address::{CellRange, CellRef, ReferenceError};
pub use column::{
    ColumnCell, ColumnError, ColumnType, ColumnTypeReport, JapaneseCheckReport, contains_japanese,
};
pub use document::{
    CsvDocument, DocumentError, ExternalCellConflict, ExternalConflictDraft,
    ExternalStructureConflict,
};
pub use validation::{
    ValidationComparisonOperator, ValidationExpression, ValidationOperand, ValidationReport,
    ValidationRule, ValidationTarget, ValidationViolation,
};
#[cfg(feature = "gui")]
pub(crate) use watcher::CsvFileWatcher;
