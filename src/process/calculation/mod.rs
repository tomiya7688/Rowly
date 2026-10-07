//! ```text
//! 責務: [calculation: CSVの外でpure calculation bindingとderived resultを管理する]
//! 処理: [1: bindingと結果の型を公開する 2: dependency graphを評価するengineを公開する]
//! 補足: [このmoduleはCSVを変更せず、式は固定された副作用なしASTだけで表現する]
//! ```
mod engine;
mod expression;
mod graph;
mod indexes;
mod types;

pub use engine::CalculationEngine;
pub use types::{
    CalculationBinding, CalculationBindingId, CalculationDependencySnapshot, CalculationError,
    CalculationExpression, CalculationFailure, CalculationOperator, CalculationRecalculationReport,
    CalculationResult, CalculationStatus, CalculationTarget, CalculationTrigger, CalculationValue,
    PureCalculationFunction,
};

#[cfg(test)]
mod tests;
