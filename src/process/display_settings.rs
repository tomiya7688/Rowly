//! ```text
//! 責務: [display_settings: 列の表示書式・View style・条件付き書式を1つの設定としてまとめ、現在のCSVの列へ解決し、cellの表示（表示文字列・効くstyle・診断）を一度に求める]
//! 処理: [1: 列の表示書式は見出しで持つ 2: 解決時に見出しが無い・重複する設定だけ休眠にし、他の設定はそのまま使う 3: cellには、表示書式で整形した文字列と、View styleの上に条件付き書式を重ねたstyleを返す]
//! 補足: [すべてCSVの外のView設定で、CSVには触れない。休眠した設定は捨てず、見出しが戻れば再び有効になる。保存形式（generated init DSL）はproject_init側が扱う]
//! ```
use super::conditional_format::{
    CellEvaluation, CellInput, ConditionalFormat, ResolvedConditional,
};
use super::display_format::{CustomFormatRegistry, Display, DisplayFormat, format_value_with};
use super::view_style::{CellStyle, DormantRule, ResolvedStyle, ViewStyle};

/// 1つの列の表示書式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnFormat {
    /// 列の見出し。
    pub column: String,
    /// 表示書式。
    pub format: DisplayFormat,
}

/// 表示に関する設定一式。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplaySettings {
    /// 列ごとの表示書式。同じ見出しは1つだけ。
    pub formats: Vec<ColumnFormat>,
    /// View style。
    pub style: ViewStyle,
    /// 条件付き書式。
    pub conditional: ConditionalFormat,
}

/// 現在のCSVへ解決した表示設定。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedDisplay {
    formats: Vec<(usize, DisplayFormat)>,
    /// 休眠した列の表示書式（現在のCSVに無い・重複する見出し）。
    pub dormant_formats: Vec<ColumnFormat>,
    /// 解決したView style。
    pub style: ResolvedStyle,
    /// 解決した条件付き書式。
    pub conditional: ResolvedConditional,
}

/// 1 cellの表示結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedCell {
    /// 表示書式を適用した表示。書式に読めなければrawのままのerror。
    pub display: Display,
    /// 効くstyle。View styleの上に、合った条件付き書式を重ねたもの。
    pub style: CellStyle,
    /// 条件付き書式の評価結果（合ったruleと、評価できなかった理由）。
    pub conditional: CellEvaluation,
}

impl DisplaySettings {
    // {
    //   責務: [resolve: 現在の見出しへ設定を引き直す]
    //   処理: [列の表示書式は見出しが1つに決まるものだけ有効にし、無い・重複するものは休眠として残す。View styleと条件付き書式は、それぞれのresolveに任せる]
    //   引数: [headers: 現在のCSVの見出し行]
    // }
    pub fn resolve(&self, headers: &[String]) -> ResolvedDisplay {
        let mut formats = Vec::new();
        let mut dormant_formats = Vec::new();
        for entry in &self.formats {
            let mut found = headers
                .iter()
                .enumerate()
                .filter(|(_, header)| **header == entry.column);
            match (found.next(), found.next()) {
                (Some((column, _)), None) => formats.push((column, entry.format.clone())),
                _ => dormant_formats.push(entry.clone()),
            }
        }
        ResolvedDisplay {
            formats,
            dormant_formats,
            style: self.style.resolve(headers),
            conditional: self.conditional.resolve(headers),
        }
    }
}

impl ResolvedDisplay {
    /// 休眠したView styleのrule。
    pub fn dormant_style_rules(&self) -> &[DormantRule] {
        &self.style.dormant
    }

    /// 休眠した条件付き書式のrule。
    pub fn dormant_conditional_rules(&self) -> &[DormantRule] {
        &self.conditional.dormant
    }

    // {
    //   責務: [render_cell: 1 cellの表示文字列と効くstyleを求める]
    //   処理: [列に表示書式があれば整形する（無ければrawのまま）。styleはView styleを土台に、条件付き書式のstyleを重ねる。条件付き書式の評価はrawと計算結果から行い、整形後の文字列は使わない]
    //   引数: [row: CSVの行位置, column: 列位置, input: raw文字列と計算結果, custom: Custom書式の登録先]
    // }
    pub fn render_cell(
        &self,
        row: usize,
        column: usize,
        input: &CellInput<'_>,
        custom: &dyn CustomFormatRegistry,
    ) -> RenderedCell {
        let display = match self.formats.iter().find(|(index, _)| *index == column) {
            Some((_, format)) => format_value_with(input.raw, format, custom),
            None => Display::Formatted(input.raw.to_owned()),
        };
        let conditional = self.conditional.evaluate(row, column, input);
        let style = self
            .style
            .style_for(row, column)
            .merged_with(&conditional.style);
        RenderedCell {
            display,
            style,
            conditional,
        }
    }
}
