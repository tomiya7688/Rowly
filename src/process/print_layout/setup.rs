//! ```text
//! 責務: [print_layout::setup: Page Setupのmodel（向き・用紙・余白・倍率・ページ順・ヘッダー/フッター）を持つ]
//! 補足: [Page SetupはCSVの正本ではなくView / print stateである。長さの単位はmmで、計算はprint_layout側が整数のマイクロメートルで行う]
//! ```

/// 用紙の向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// 縦。
    Portrait,
    /// 横。
    Landscape,
}

/// 用紙サイズ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaperSize {
    /// A3。
    A3,
    /// A4。
    A4,
    /// A5。
    A5,
    /// B4（JIS）。
    B4,
    /// B5（JIS）。
    B5,
    /// Letter。
    Letter,
    /// Legal。
    Legal,
    /// mm指定の用紙（幅、高さ。縦向きでの値）。
    Custom {
        /// 幅（mm）。
        width_mm: f64,
        /// 高さ（mm）。
        height_mm: f64,
    },
}

impl PaperSize {
    // {
    //   責務: [dimensions_mm: 縦向きの用紙の幅と高さ（mm）を返す]
    // }
    pub(super) fn dimensions_mm(self) -> (f64, f64) {
        match self {
            Self::A3 => (297.0, 420.0),
            Self::A4 => (210.0, 297.0),
            Self::A5 => (148.0, 210.0),
            Self::B4 => (257.0, 364.0),
            Self::B5 => (182.0, 257.0),
            Self::Letter => (215.9, 279.4),
            Self::Legal => (215.9, 355.6),
            Self::Custom {
                width_mm,
                height_mm,
            } => (width_mm, height_mm),
        }
    }
}

/// 余白（mm）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margins {
    /// 上。
    pub top_mm: f64,
    /// 下。
    pub bottom_mm: f64,
    /// 左。
    pub left_mm: f64,
    /// 右。
    pub right_mm: f64,
}

impl Margins {
    /// 全辺が同じ余白。
    pub fn uniform(mm: f64) -> Self {
        Self {
            top_mm: mm,
            bottom_mm: mm,
            left_mm: mm,
            right_mm: mm,
        }
    }
}

/// 倍率の指定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// 固定倍率（パーセント。10〜400）。
    Percent(u16),
    /// 幅に合わせる。表が1ページの幅に収まるよう縮小する。拡大はしない。
    FitWidth,
}

/// ページを進める順序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageOrder {
    /// 下へ進み、次の列グループへ移る。
    DownThenOver,
    /// 右へ進み、次の行グループへ移る。
    OverThenDown,
}

/// ヘッダーまたはフッターの左・中央・右の文字列。`{page}` `{pages}` `{file}` `{view}`を展開できる。CSV cellには書かない。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderFooter {
    /// 左。
    pub left: String,
    /// 中央。
    pub center: String,
    /// 右。
    pub right: String,
}

/// ヘッダー/フッターの展開に使う値。
#[derive(Debug, Clone, Copy)]
pub struct HeaderFooterContext<'a> {
    /// ページ番号（1始まり）。
    pub page: usize,
    /// 総ページ数。
    pub pages: usize,
    /// file / project名。
    pub file: &'a str,
    /// View名。
    pub view: &'a str,
}

impl HeaderFooter {
    // {
    //   責務: [is_empty: 3か所とも空か判定する。空なら本文の領域を取り分けない]
    // }
    pub fn is_empty(&self) -> bool {
        self.left.is_empty() && self.center.is_empty() && self.right.is_empty()
    }

    // {
    //   責務: [expand: 左・中央・右のtokenを値に置き換える]
    //   引数: [context: ページ番号・総ページ数・file名・View名]
    //   戻り値: [(String, String, String): 展開後の左・中央・右]
    // }
    pub fn expand(&self, context: HeaderFooterContext<'_>) -> (String, String, String) {
        let expand = |template: &str| {
            template
                .replace("{page}", &context.page.to_string())
                .replace("{pages}", &context.pages.to_string())
                .replace("{file}", context.file)
                .replace("{view}", context.view)
        };
        (
            expand(&self.left),
            expand(&self.center),
            expand(&self.right),
        )
    }
}

/// Page Setup。CSVの正本ではなく、View / print stateである。
#[derive(Debug, Clone, PartialEq)]
pub struct PageSetup {
    /// 向き。
    pub orientation: Orientation,
    /// 用紙。
    pub paper: PaperSize,
    /// 余白。
    pub margins: Margins,
    /// 倍率。
    pub scale: Scale,
    /// 見出し行を2ページ目以降で繰り返すか。
    pub repeat_header_row: bool,
    /// ページ順。
    pub page_order: PageOrder,
    /// ヘッダー。
    pub header: HeaderFooter,
    /// フッター。
    pub footer: HeaderFooter,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self {
            orientation: Orientation::Portrait,
            paper: PaperSize::A4,
            margins: Margins::uniform(15.0),
            scale: Scale::Percent(100),
            repeat_header_row: true,
            page_order: PageOrder::DownThenOver,
            header: HeaderFooter::default(),
            footer: HeaderFooter::default(),
        }
    }
}
