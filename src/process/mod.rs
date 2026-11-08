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
pub mod aggregation;
pub mod aggregation_definition;
pub mod calculation;
pub mod chart;
mod clipboard;
mod column;
pub mod column_reshape;
pub mod conditional_format;
pub mod diff;
pub mod display_format;
pub mod display_settings;
mod document;
pub mod fill;
mod history;
mod metadata;
pub mod paste_special;
pub mod pivot;
pub mod print_layout;
pub mod profile;
pub mod remove_duplicates;
pub mod sort_rows;
pub mod text_transform;
mod validation;
pub mod view_style;
#[cfg(feature = "gui")]
mod watcher;

pub use crate::data::SourceEncoding;
pub use address::{CellRange, CellRef, ReferenceError};
pub use column::{
    ColumnCell, ColumnError, ColumnType, ColumnTypeReport, JapaneseCheckReport, contains_japanese,
};
pub use column_reshape::{CombinePlan, SplitPlan};
pub use diff::{
    Alignment, CellMerge, DiffChange, DiffKind, MergeKind, MergeSide, TableDiff, ThreeWayDiff,
    diff_tables, diff_three_way,
};
pub use document::{
    CsvDocument, DocumentError, ExternalCellConflict, ExternalConflictDraft,
    ExternalStructureConflict,
};
pub use fill::{DecimalSeries, FillDirection, FillPlan, IntegerSeries, SeriesGenerator};
pub use paste_special::{ClipboardGrid, PasteMode, RaggedPolicy};
pub use profile::{
    ColumnProfile, FrequentValue, NumericRange, ProfileAccumulator, ProfileOptions, ProfileScope,
};
pub use remove_duplicates::{DuplicatePlan, KeepPolicy, RemovedRow};
pub use sort_rows::{RowOrderPlan, SortDirection, SortKey, SortKind, reject_multi_source};
pub use text_transform::{
    CaseMode, PlannedChange, TextTransform, TextTransformPlan, TransformTarget, TrimMode,
    apply_text_transform,
};
pub use validation::{
    ValidationComparisonOperator, ValidationExpression, ValidationOperand, ValidationReport,
    ValidationRule, ValidationTarget, ValidationViolation,
};
#[cfg(feature = "gui")]
pub(crate) use watcher::CsvFileWatcher;
