//! 構造編集後のcell参照を新しい位置へ付け替える。

use super::*;

impl CalculationExpression {
    /// {
    ///   責務: [remap_cell_references: 行・列の構造編集に合わせて全cell参照を同じ論理cellへ付け替える]
    ///   処理: [相対・絶対の別なく参照先の移動に追従し、削除された参照先はDeletedReferenceにする]
    ///   引数: [map: 旧位置を新位置へ写す関数。削除された位置はNone]
    ///   戻り値: [Self: 付け替え後の式]
    /// }
    pub fn remap_cell_references(
        &self,
        map: &impl Fn(CellRef) -> Option<CellRef>,
        map_range: &impl Fn(CellRef, CellRef) -> Option<(CellRef, CellRef)>,
    ) -> Self {
        match self {
            Self::Literal(value) => Self::Literal(value.clone()),
            Self::Cell(reference) => map(*reference).map_or(Self::DeletedReference, Self::Cell),
            Self::AnchoredCell(reference) => map(reference.cell())
                .map_or(Self::DeletedReference, |cell| {
                    Self::AnchoredCell(reference.with_cell(cell))
                }),
            Self::DeletedReference => Self::DeletedReference,
            Self::Aggregate { function, range } => {
                map_range(range.start().cell(), range.end().cell())
                    .and_then(|(start, end)| range.with_cells(start, end))
                    .map_or(Self::DeletedReference, |range| Self::Aggregate {
                        function: *function,
                        range,
                    })
            }
            Self::Negate(operand) => {
                Self::Negate(Box::new(operand.remap_cell_references(map, map_range)))
            }
            Self::Arithmetic {
                left,
                operator,
                right,
            } => Self::Arithmetic {
                left: Box::new(left.remap_cell_references(map, map_range)),
                operator: *operator,
                right: Box::new(right.remap_cell_references(map, map_range)),
            },
            Self::Call {
                function,
                arguments,
            } => Self::Call {
                function: *function,
                arguments: arguments
                    .iter()
                    .map(|argument| argument.remap_cell_references(map, map_range))
                    .collect(),
            },
        }
    }
}

impl CalculationExpression {
    /// {
    ///   責務: [has_oversized_range: 展開上限MAX_RANGE_CELLSを超えるrange参照を含むかを返す]
    ///   戻り値: [bool: 上限超過のrangeがあればtrue]
    /// }
    pub fn has_oversized_range(&self) -> bool {
        let mut pending = vec![self];
        while let Some(expression) = pending.pop() {
            match expression {
                Self::Aggregate { range, .. } if range.cell_count() > MAX_RANGE_CELLS => {
                    return true;
                }
                Self::Negate(operand) => pending.push(operand),
                Self::Arithmetic { left, right, .. } => {
                    pending.push(left);
                    pending.push(right);
                }
                Self::Call { arguments, .. } => pending.extend(arguments.iter()),
                _ => {}
            }
        }
        false
    }
}
