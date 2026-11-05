//! 構造編集後のcell参照を新しい位置へ付け替える。

use super::*;

impl CalculationExpression {
    /// {
    ///   責務: [remap_cell_references: 行・列の構造編集に合わせて全cell参照を同じ論理cellへ付け替える]
    ///   処理: [相対・絶対の別なく参照先の移動に追従し、削除された参照先はDeletedReferenceにする]
    ///   引数: [map: 旧位置を新位置へ写す関数。削除された位置はNone]
    ///   戻り値: [Self: 付け替え後の式]
    /// }
    pub fn remap_cell_references(&self, map: &impl Fn(CellRef) -> Option<CellRef>) -> Self {
        match self {
            Self::Literal(value) => Self::Literal(value.clone()),
            Self::Cell(reference) => map(*reference).map_or(Self::DeletedReference, Self::Cell),
            Self::AnchoredCell(reference) => map(reference.cell())
                .map_or(Self::DeletedReference, |cell| {
                    Self::AnchoredCell(reference.with_cell(cell))
                }),
            Self::DeletedReference => Self::DeletedReference,
            Self::Negate(operand) => Self::Negate(Box::new(operand.remap_cell_references(map))),
            Self::Arithmetic {
                left,
                operator,
                right,
            } => Self::Arithmetic {
                left: Box::new(left.remap_cell_references(map)),
                operator: *operator,
                right: Box::new(right.remap_cell_references(map)),
            },
            Self::Call {
                function,
                arguments,
            } => Self::Call {
                function: *function,
                arguments: arguments
                    .iter()
                    .map(|argument| argument.remap_cell_references(map))
                    .collect(),
            },
        }
    }
}
