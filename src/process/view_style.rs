//! ```text
//! 責務: [view_style: cell・範囲・列に付けるView style（配置・折返し・罫線・太字等・色・行高）のmodelと、決定的な統合・解決を提供する]
//! 処理: [1: styleは「指定した項目だけ」を持つ（未指定は上書きしない） 2: 複数のruleは入力順に、項目ごとに後の明示値で統合する 3: targetは列の見出しとCSVの行位置で持ち、現在の見出しへ解決する。見出しが無いruleだけが休眠する]
//! 補足: [styleはCSVの外のView設定で、CSVには触れない。罫線は6方向（斜め2方向を含む）を同じ操作で扱い、斜め罫線だけ別の経路にしない]
//! ```
use std::collections::BTreeMap;

/// 水平方向の配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalAlign {
    /// 左。
    Left,
    /// 中央。
    Center,
    /// 右。
    Right,
}

/// 垂直方向の配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlign {
    /// 上。
    Top,
    /// 中央。
    Middle,
    /// 下。
    Bottom,
}

/// 長い文字列の扱い。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOverflow {
    /// 折り返す。
    Wrap,
    /// 切り詰める。
    Truncate,
    /// 切り詰めて省略記号を付ける。
    Ellipsis,
}

/// 罫線の位置。斜めも含めて6方向を同じ型・同じ操作で扱う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BorderEdge {
    /// 上。
    Top,
    /// 下。
    Bottom,
    /// 左。
    Left,
    /// 右。
    Right,
    /// 左上から右下への斜線。
    DiagonalDown,
    /// 左下から右上への斜線。
    DiagonalUp,
}

impl BorderEdge {
    /// 全6方向。キーボード操作などが方向を網羅するときの正本。
    pub const ALL: [BorderEdge; 6] = [
        BorderEdge::Top,
        BorderEdge::Bottom,
        BorderEdge::Left,
        BorderEdge::Right,
        BorderEdge::DiagonalDown,
        BorderEdge::DiagonalUp,
    ];
}

/// 罫線の線種。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
    /// 細い実線。
    Thin,
    /// 中くらいの実線。
    Medium,
    /// 太い実線。
    Thick,
    /// 破線。
    Dashed,
    /// 点線。
    Dotted,
    /// 二重線。
    Double,
}

/// 色（`#rrggbb`、小文字へ正規化）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Color(String);

impl Color {
    // {
    //   責務: [parse: `#rrggbb`の文字列を色にする。大文字は小文字へ正規化する]
    //   戻り値: [Option<Color>: 形式が合えばSome]
    // }
    pub fn parse(text: &str) -> Option<Self> {
        let valid = text.len() == 7
            && text.starts_with('#')
            && text[1..].bytes().all(|byte| byte.is_ascii_hexdigit());
        valid.then(|| Self(text.to_ascii_lowercase()))
    }

    /// `#rrggbb`の文字列。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 1辺の罫線。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorderLine {
    /// 線種。
    pub style: BorderStyle,
    /// 色。Noneなら既定の色。
    pub color: Option<Color>,
}

/// cellのstyle。各項目は「未指定（None）」を持ち、未指定は他のstyleを上書きしない。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellStyle {
    /// 水平配置。
    pub horizontal: Option<HorizontalAlign>,
    /// 垂直配置。
    pub vertical: Option<VerticalAlign>,
    /// 長い文字列の扱い。
    pub overflow: Option<TextOverflow>,
    /// 太字。
    pub bold: Option<bool>,
    /// 斜体。
    pub italic: Option<bool>,
    /// 下線。
    pub underline: Option<bool>,
    /// 文字色。
    pub text_color: Option<Color>,
    /// 背景色。
    pub background: Option<Color>,
    /// 罫線。指定した辺だけを持つ。辺ごとの明示的な「罫線なし」は`BorderEdgeSetting::Clear`で表す。
    pub borders: BTreeMap<BorderEdge, BorderEdgeSetting>,
}

/// 1辺の罫線の指定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BorderEdgeSetting {
    /// この罫線を付ける。
    Line(BorderLine),
    /// この辺の罫線を明示的に消す（他のruleで付いた罫線も消す）。
    Clear,
}

impl CellStyle {
    // {
    //   責務: [set_border: 6方向のどの辺も同じ操作で罫線を指定する。斜め罫線だけ別の経路を持たない]
    //   引数: [edge: 辺, setting: 罫線の指定]
    // }
    pub fn set_border(&mut self, edge: BorderEdge, setting: BorderEdgeSetting) {
        self.borders.insert(edge, setting);
    }

    // {
    //   責務: [merged_with: このstyleの上に`over`を重ねる。項目ごとに`over`の明示値が勝ち、未指定の項目はこのstyleのまま]
    //   戻り値: [CellStyle: 統合結果。入力が同じなら常に同じ]
    // }
    pub fn merged_with(&self, over: &CellStyle) -> CellStyle {
        let mut borders = self.borders.clone();
        for (edge, setting) in &over.borders {
            borders.insert(*edge, setting.clone());
        }
        CellStyle {
            horizontal: over.horizontal.or(self.horizontal),
            vertical: over.vertical.or(self.vertical),
            overflow: over.overflow.or(self.overflow),
            bold: over.bold.or(self.bold),
            italic: over.italic.or(self.italic),
            underline: over.underline.or(self.underline),
            text_color: over.text_color.clone().or_else(|| self.text_color.clone()),
            background: over.background.clone().or_else(|| self.background.clone()),
            borders,
        }
    }
}

/// styleを付ける対象。列は見出し、行はCSVの行位置（0始まり。表示順ではない）で指す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleTarget {
    /// 1つの列の全cell。
    Column(String),
    /// 1つのcell。
    Cell {
        /// 列の見出し。
        column: String,
        /// CSVの行位置。
        row: usize,
    },
    /// 列の集合と行の範囲（両端を含む）。
    Range {
        /// 列の見出し。
        columns: Vec<String>,
        /// 最初の行位置。
        first_row: usize,
        /// 最後の行位置。
        last_row: usize,
    },
}

/// 1つのstyle rule。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleRule {
    /// 対象。
    pub target: StyleTarget,
    /// 付けるstyle。
    pub style: CellStyle,
}

/// 行高の上限（mm）。
const MAX_ROW_HEIGHT_MM: f64 = 1_000.0;

/// View style全体。ruleは入力順に適用する。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewStyle {
    /// style rule。後のruleが項目ごとに先のruleを上書きする。
    pub rules: Vec<StyleRule>,
    /// 行高（mm）。キーはCSVの行位置。
    pub row_heights: BTreeMap<usize, f64>,
}

/// styleの指定の不備。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ViewStyleError {
    /// targetが空、または範囲が逆。
    #[error("style target is empty or its row range is reversed")]
    InvalidTarget,
    /// 行高が正で有限な範囲にない。
    #[error("row height must be a positive finite length up to 1000 mm")]
    InvalidRowHeight,
}

impl ViewStyle {
    // {
    //   責務: [add_rule: ruleを末尾へ加える。targetが空・逆の範囲ならerror]
    // }
    pub fn add_rule(&mut self, rule: StyleRule) -> Result<(), ViewStyleError> {
        let valid = match &rule.target {
            StyleTarget::Column(header) | StyleTarget::Cell { column: header, .. } => {
                !header.is_empty()
            }
            StyleTarget::Range {
                columns,
                first_row,
                last_row,
            } => !columns.is_empty() && first_row <= last_row,
        };
        if !valid {
            return Err(ViewStyleError::InvalidTarget);
        }
        self.rules.push(rule);
        Ok(())
    }

    // {
    //   責務: [set_row_height: 行高を設定する。正で有限、上限以下でなければerror]
    // }
    pub fn set_row_height(&mut self, row: usize, height_mm: f64) -> Result<(), ViewStyleError> {
        if !height_mm.is_finite() || height_mm <= 0.0 || height_mm > MAX_ROW_HEIGHT_MM {
            return Err(ViewStyleError::InvalidRowHeight);
        }
        self.row_heights.insert(row, height_mm);
        Ok(())
    }

    // {
    //   責務: [resolve: 現在の見出しへtargetを引き直す]
    //   処理: [各ruleの列の見出しを探す。1つでも無い・複数あるruleだけを休眠（dormant）にして理由を返し、他のruleは有効のまま。休眠したruleは捨てず、見出しが戻れば再び有効になる]
    //   引数: [headers: 現在のCSVの見出し行]
    //   戻り値: [ResolvedStyle: 有効なruleと休眠したrule]
    // }
    pub fn resolve(&self, headers: &[String]) -> ResolvedStyle {
        let mut active = Vec::new();
        let mut dormant = Vec::new();
        for (index, rule) in self.rules.iter().enumerate() {
            let names: Vec<&String> = match &rule.target {
                StyleTarget::Column(header) | StyleTarget::Cell { column: header, .. } => {
                    vec![header]
                }
                StyleTarget::Range { columns, .. } => columns.iter().collect(),
            };
            let mut columns = Vec::new();
            let mut missing = Vec::new();
            let mut ambiguous = Vec::new();
            for name in names {
                let mut found = headers
                    .iter()
                    .enumerate()
                    .filter(|(_, header)| *header == name);
                match (found.next(), found.next()) {
                    (Some((column, _)), None) => {
                        // 範囲に同じ見出しが2回あっても、列位置は1回だけ持つ。
                        if !columns.contains(&column) {
                            columns.push(column);
                        }
                    }
                    (None, _) => push_unique(&mut missing, name),
                    (Some(_), Some(_)) => push_unique(&mut ambiguous, name),
                }
            }
            if missing.is_empty() && ambiguous.is_empty() {
                let rows = match &rule.target {
                    StyleTarget::Column(_) => None,
                    StyleTarget::Cell { row, .. } => Some((*row, *row)),
                    StyleTarget::Range {
                        first_row,
                        last_row,
                        ..
                    } => Some((*first_row, *last_row)),
                };
                active.push(ResolvedRule {
                    index,
                    columns,
                    rows,
                    style: rule.style.clone(),
                });
            } else {
                dormant.push(DormantRule {
                    index,
                    missing,
                    ambiguous,
                });
            }
        }
        ResolvedStyle {
            active,
            dormant,
            row_heights: self.row_heights.clone(),
        }
    }
}

fn push_unique(list: &mut Vec<String>, name: &str) {
    if !list.iter().any(|existing| existing == name) {
        list.push(name.to_owned());
    }
}

/// 現在の列へ解決できたrule。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRule {
    /// 元のrule内での位置。
    pub index: usize,
    /// 対象の列位置。
    pub columns: Vec<usize>,
    /// 対象の行(両端を含む)。Noneなら全行。
    pub rows: Option<(usize, usize)>,
    /// 付けるstyle。
    pub style: CellStyle,
}

/// 現在の見出しに解決できず休眠したrule。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DormantRule {
    /// 元のrule内での位置。
    pub index: usize,
    /// 現在のCSVに無い見出し。
    pub missing: Vec<String>,
    /// 現在のCSVに複数ある見出し。
    pub ambiguous: Vec<String>,
}

/// 現在のCSVへ解決したView style。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStyle {
    /// 有効なrule(入力順)。
    pub active: Vec<ResolvedRule>,
    /// 休眠したrule。
    pub dormant: Vec<DormantRule>,
    row_heights: BTreeMap<usize, f64>,
}

impl ResolvedStyle {
    // {
    //   責務: [style_for: cellに効くstyleを決定的に統合して返す]
    //   処理: [有効なruleを入力順にたどり、そのcellを含むruleのstyleを、項目ごとに後の明示値が勝つよう重ねる]
    //   引数: [row: CSVの行位置, column: 列位置]
    //   補足: [1 cellあたり有効なruleの数に比例する計算量。大きな表では、表示するcellだけを解決して呼ぶ（全cellを走査しない）]
    //   戻り値: [CellStyle: 効くstyle。どのruleも当たらなければ全項目が未指定]
    // }
    pub fn style_for(&self, row: usize, column: usize) -> CellStyle {
        self.active
            .iter()
            .filter(|rule| {
                rule.columns.contains(&column)
                    && rule
                        .rows
                        .is_none_or(|(first, last)| (first..=last).contains(&row))
            })
            .fold(CellStyle::default(), |merged, rule| {
                merged.merged_with(&rule.style)
            })
    }

    /// 行の高さ(mm)。指定が無ければNone。
    pub fn row_height_mm(&self, row: usize) -> Option<f64> {
        self.row_heights.get(&row).copied()
    }
}
