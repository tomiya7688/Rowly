use eframe::egui;

/// {
///   責務: [HelpDocument: viewerで表示できる文書の種類を識別し、対応する見出し・file・sourceを返す。]
///   選択肢: [Reference: DSL文法。 Tutorial: DSL手順。 Bindings: 変数・定数規則。 StandardLibrary: 標準関数。]
/// }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpDocument {
    Reference,
    Tutorial,
    Bindings,
    StandardLibrary,
}

impl HelpDocument {
    // {
    //   責務: [ALL: viewerへ読み込む全HelpDocumentを列挙する。]
    // }
    const ALL: [Self; 4] = [
        Self::Reference,
        Self::Tutorial,
        Self::Bindings,
        Self::StandardLibrary,
    ];

    // {
    //   責務: [PRIMARY: navigationの主tabへ表示するhelp文書を列挙する。]
    // }
    const PRIMARY: [Self; 2] = [Self::Reference, Self::Tutorial];

    // {
    //   責務: [title: help画面で使う文書表示名を返す。]
    //   引数: [self: 表示名を求める文書種別。]
    //   戻り値: [&'static str: 文書の日本語title。]
    // }
    fn title(self) -> &'static str {
        match self {
            Self::Reference => "DSL 文法リファレンス",
            Self::Tutorial => "DSL チュートリアル",
            Self::Bindings => "DSL の変数・定数・再代入",
            Self::StandardLibrary => "DSL 標準関数",
        }
    }

    // {
    //   責務: [file_name: 文書のlink解決とheader表示に使うfile名を返す。]
    //   引数: [self: file名を求める文書種別。]
    //   戻り値: [&'static str: docs配下のMarkdown file名。]
    // }
    fn file_name(self) -> &'static str {
        match self {
            Self::Reference => "DSL_REFERENCE.md",
            Self::Tutorial => "DSL_TUTORIAL.md",
            Self::Bindings => "DSL_BINDINGS.md",
            Self::StandardLibrary => "DSL_STANDARD_LIBRARY.md",
        }
    }

    // {
    //   責務: [source: compile時に埋め込んだMarkdown本文を返す。]
    //   引数: [self: sourceを求める文書種別。]
    //   戻り値: [&'static str: 対応するdocs Markdown本文。]
    // }
    fn source(self) -> &'static str {
        match self {
            Self::Reference => include_str!("../docs/DSL_REFERENCE.md"),
            Self::Tutorial => include_str!("../docs/DSL_TUTORIAL.md"),
            Self::Bindings => include_str!("../docs/DSL_BINDINGS.md"),
            Self::StandardLibrary => include_str!("../docs/DSL_STANDARD_LIBRARY.md"),
        }
    }
}

/// {
///   責務: [HelpTarget: help viewerで開く文書と見出しanchorを指定する。]
///   フィールド: [document: 表示する文書種別。 anchor: 開始位置として解決するanchorまたは見出し名。]
/// }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelpTarget {
    pub document: HelpDocument,
    pub anchor: String,
}

impl HelpTarget {
    /// {
    ///   責務: [new: 文書とanchorからHelpTargetを作成する。]
    ///   処理: [anchor入力を所有Stringへ変換してfieldへ保存する。]
    ///   引数: [document: 開く文書。 anchor: 開始位置のanchorまたは見出し名。]
    ///   戻り値: [HelpTarget: 指定文書の開始位置。]
    /// }
    pub fn new(document: HelpDocument, anchor: impl Into<String>) -> Self {
        Self {
            document,
            anchor: anchor.into(),
        }
    }
}

// {
//   責務: [HelpSection: 解析済み見出しの識別子・表示名・階層・本文を保持する。]
//   フィールド: [anchor: link/scroll用id。 title: 表示heading。 level: Markdown heading階層。 body: 見出しから次見出しまでの本文。]
// }
#[derive(Debug)]
struct HelpSection {
    anchor: String,
    title: String,
    level: usize,
    body: String,
}

// {
//   責務: [HelpPage: HelpDocumentの見出しsectionを解析して保持する。]
//   フィールド: [document: 元文書種別。 sections: 順番を保った解析済みsection。]
// }
#[derive(Debug)]
struct HelpPage {
    document: HelpDocument,
    sections: Vec<HelpSection>,
}

impl HelpPage {
    // {
    //   責務: [new: HelpDocumentのsourceをsection一覧へ解析してHelpPageを作成する。]
    //   処理: [document.sourceをparse_sectionsへ渡してsectionを格納する。]
    //   引数: [document: 解析対象の文書種別。]
    //   戻り値: [HelpPage: documentと解析済み見出し一覧。]
    // }
    fn new(document: HelpDocument) -> Self {
        Self {
            document,
            sections: parse_sections(document.source()),
        }
    }

    // {
    //   責務: [first_anchor: 最初のsectionのanchorを返す。]
    //   引数: [self: anchor一覧を保持するpage。]
    //   戻り値: [&str: 先頭anchor。sectionがない場合は空文字列。]
    // }
    fn first_anchor(&self) -> &str {
        self.sections
            .first()
            .map_or("", |section| section.anchor.as_str())
    }

    // {
    //   責務: [anchor_for: requested valueに一致するsection anchorを解決する。]
    //   処理: [anchorの完全一致またはslug化したtitleの大文字小文字を無視した一致を探す。]
    //   引数: [self: 検索対象section一覧。 requested: anchorまたはheading名。]
    //   戻り値: [Option<&str>: 一致したsection anchor。見つからなければNone。]
    // }
    fn anchor_for(&self, requested: &str) -> Option<&str> {
        self.sections
            .iter()
            .find(|section| {
                section.anchor == requested
                    || slugify(&section.title).eq_ignore_ascii_case(requested)
            })
            .map(|section| section.anchor.as_str())
    }
}

/// {
///   責務: [HelpViewer: DSL文書のnavigation・見出し検索・本文表示・内部link移動状態を管理する。]
///   フィールド: [pages: 解析済み文書。 active_document: 表示中文書。 selected_anchor: 選択見出し。 heading_filter: 見出し検索入力。 pending_scroll: 次回表示時に移動するanchor。]
/// }
#[derive(Debug)]
pub struct HelpViewer {
    pages: Vec<HelpPage>,
    active_document: HelpDocument,
    selected_anchor: String,
    heading_filter: String,
    pending_scroll: Option<String>,
}

impl Default for HelpViewer {
    // {
    //   責務: [default: 全help文書を読み込み、referenceの先頭sectionを表示する初期viewerを作る。]
    //   処理: [HelpDocument::ALLからpageを構築し、active documentのfirst anchorを選択する。]
    //   引数: []
    //   戻り値: [HelpViewer: referenceを初期表示するviewer state。]
    // }
    fn default() -> Self {
        let pages = HelpDocument::ALL
            .into_iter()
            .map(HelpPage::new)
            .collect::<Vec<_>>();
        let active_document = HelpDocument::Reference;
        let selected_anchor = pages
            .iter()
            .find(|page| page.document == active_document)
            .map_or_else(String::new, |page| page.first_anchor().to_owned());
        Self {
            pages,
            active_document,
            selected_anchor,
            heading_filter: String::new(),
            pending_scroll: None,
        }
    }
}

impl HelpViewer {
    /// {
    ///   責務: [open: 指定targetへviewerのactive documentと見出し選択を移す。]
    ///   処理: [文書内でanchorまたはheading名を解決し、見つからない場合は先頭anchorを選んで検索filterをclearする。]
    ///   引数: [self: 更新対象のviewer state。 target: 開く文書と開始位置。]
    ///   戻り値: [(): 選択位置と次回scroll先を更新する。]
    /// }
    pub fn open(&mut self, target: HelpTarget) {
        let page = self.page(target.document);
        let anchor = page
            .anchor_for(&target.anchor)
            .unwrap_or_else(|| page.first_anchor())
            .to_owned();
        self.active_document = target.document;
        self.selected_anchor.clone_from(&anchor);
        self.heading_filter.clear();
        self.pending_scroll = Some(anchor);
    }

    /// {
    ///   責務: [show: help navigationと選択文書のMarkdown内容をeguiへ描画する。]
    ///   処理: [document・見出しfilter・anchorを編集可能にし、navigation・section scroll・内部link移動を処理する。]
    ///   引数: [self: 文書・selection・filter・scroll stateを保持するviewer。 ctx: 表示用egui context。]
    ///   戻り値: [bool: worksheetへ戻る操作が選択された場合true。]
    ///   副作用: [navigation選択と内部link操作に応じてviewer stateを更新する。]
    /// }
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let mut close_requested = false;
        let mut requested_document = None;
        let mut requested_anchor = None;
        let current_document = self.active_document;
        let mut filter = self.heading_filter.clone();
        let mut selected_anchor = self.selected_anchor.clone();
        let headings = self
            .page(current_document)
            .sections
            .iter()
            .map(|section| (section.anchor.clone(), section.title.clone(), section.level))
            .collect::<Vec<_>>();

        egui::SidePanel::left("help_navigation")
            .default_width(265.0)
            .show(ctx, |ui| {
                ui.heading("Documentation");
                ui.horizontal_wrapped(|ui| {
                    for document in HelpDocument::PRIMARY {
                        if ui
                            .selectable_label(current_document == document, document.title())
                            .clicked()
                        {
                            requested_document = Some(document);
                        }
                    }
                });
                ui.collapsing("関連するDSL文書", |ui| {
                    for document in [HelpDocument::Bindings, HelpDocument::StandardLibrary] {
                        if ui
                            .selectable_label(current_document == document, document.title())
                            .clicked()
                        {
                            requested_document = Some(document);
                        }
                    }
                });
                ui.separator();
                ui.label("見出しを絞り込む");
                ui.add(
                    egui::TextEdit::singleline(&mut filter)
                        .hint_text("見出し名またはanchor")
                        .desired_width(f32::INFINITY),
                );
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("help_heading_list")
                    .show(ui, |ui| {
                        for (anchor, title, level) in &headings {
                            if !filter.is_empty()
                                && !title.to_lowercase().contains(&filter.to_lowercase())
                                && !anchor.to_lowercase().contains(&filter.to_lowercase())
                            {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                ui.add_space((level.saturating_sub(1) * 12) as f32);
                                if ui
                                    .selectable_label(selected_anchor == *anchor, title)
                                    .clicked()
                                {
                                    selected_anchor.clone_from(anchor);
                                    requested_anchor = Some(anchor.clone());
                                }
                            });
                        }
                    });
                ui.separator();
                if ui.button("ワークシートへ戻る").clicked() {
                    close_requested = true;
                }
            });

        self.heading_filter = filter;
        if let Some(document) = requested_document {
            let first_anchor = self.page(document).first_anchor().to_owned();
            self.active_document = document;
            selected_anchor.clone_from(&first_anchor);
            self.pending_scroll = Some(first_anchor);
            self.heading_filter.clear();
        } else if let Some(anchor) = requested_anchor {
            self.pending_scroll = Some(anchor);
        }
        self.selected_anchor = selected_anchor.clone();
        if close_requested {
            return true;
        }

        let document = self.active_document;
        let pending_scroll = self.pending_scroll.take();
        let sections = &self.page(document).sections;
        let mut linked_target = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(document.title());
                ui.separator();
                ui.label(format!("{} · #{}", document.file_name(), selected_anchor));
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("help_markdown_content")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for section in sections {
                        let heading_size = match section.level {
                            1 => 25.0,
                            2 => 21.0,
                            3 => 18.0,
                            _ => 16.0,
                        };
                        let heading = ui.add(
                            egui::Label::new(
                                egui::RichText::new(&section.title)
                                    .size(heading_size)
                                    .strong(),
                            )
                            .selectable(false),
                        );
                        if pending_scroll.as_deref() == Some(section.anchor.as_str()) {
                            ui.scroll_to_rect(heading.rect, Some(egui::Align::Min));
                        }
                        ui.weak(format!("#{}", section.anchor));
                        render_markdown_body(ui, &section.body, document, &mut linked_target);
                        ui.add_space(14.0);
                    }
                });
        });
        if let Some(target) = linked_target {
            self.open(target);
        }
        false
    }

    // {
    //   責務: [page: document種別に対応する読み込み済みHelpPageを取得する。]
    //   引数: [self: HelpPage一覧を保持するviewer。 document: 取得する文書種別。]
    //   戻り値: [&HelpPage: 対応する解析済みpage。]
    //   補足: [全HelpDocumentをdefault時に読み込むため、未登録は不変条件違反として扱う。]
    // }
    fn page(&self, document: HelpDocument) -> &HelpPage {
        self.pages
            .iter()
            .find(|page| page.document == document)
            .expect("all help documents are loaded")
    }
}

// {
//   責務: [parse_sections: Markdown sourceから見出し単位のHelpSection一覧を生成する。]
//   処理: [fenced code block外のexplicit anchorとheadingを認識し、本文を直前sectionへ蓄積する。重複anchorにはsuffixを付ける。]
//   引数: [source: 分割対象のMarkdown text。]
//   戻り値: [Vec<HelpSection>: 出現順の見出し・anchor・本文一覧。]
// }
fn parse_sections(source: &str) -> Vec<HelpSection> {
    let mut sections = Vec::new();
    let mut current: Option<HelpSection> = None;
    let mut pending_anchor = None;
    let mut in_code_block = false;
    let mut used_anchors = Vec::<String>::new();

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            if let Some(section) = current.as_mut() {
                section.body.push_str(line);
                section.body.push('\n');
            }
            continue;
        }
        if !in_code_block {
            if let Some(anchor) = parse_explicit_anchor(trimmed) {
                pending_anchor = Some(anchor.to_owned());
                continue;
            }
            if let Some((level, title)) = parse_heading(trimmed) {
                if let Some(section) = current.take() {
                    sections.push(section);
                }
                let mut anchor = pending_anchor.take().unwrap_or_else(|| slugify(title));
                let base = anchor.clone();
                let mut suffix = 2;
                while used_anchors.contains(&anchor) {
                    anchor = format!("{base}-{suffix}");
                    suffix += 1;
                }
                used_anchors.push(anchor.clone());
                current = Some(HelpSection {
                    anchor,
                    title: clean_inline_markup(title),
                    level,
                    body: String::new(),
                });
                continue;
            }
        }
        if let Some(section) = current.as_mut() {
            section.body.push_str(line);
            section.body.push('\n');
        }
    }
    if let Some(section) = current {
        sections.push(section);
    }
    sections
}

// {
//   責務: [parse_explicit_anchor: 対応するHTML anchor行からidを取り出す。]
//   引数: [line: trim済みのMarkdown line。]
//   戻り値: [Option<&str>: `<a id="..."`形式ならid部分、その他はNone。]
// }
fn parse_explicit_anchor(line: &str) -> Option<&str> {
    line.strip_prefix("<a id=\"")?.strip_suffix("\"></a>")
}

// {
//   責務: [parse_heading: Markdown ATX headingからlevelとtitleを解析する。]
//   処理: [先頭#の数と後続spaceを検証し、1〜6 levelのtitleをtrimして返す。]
//   引数: [line: trim済みの候補line。]
//   戻り値: [Option<(usize, &str)>: heading levelとtitle。heading形式でなければNone。]
// }
fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let level = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if level == 0 || level > 6 || line.as_bytes().get(level) != Some(&b' ') {
        return None;
    }
    Some((level, line[level + 1..].trim()))
}

// {
//   責務: [slugify: heading titleをURL風anchor textへ変換する。]
//   処理: [小文字化し、alphanumericとunderscoreを保持し、空白・hyphenの連続を単一dashへ畳む。]
//   引数: [title: anchor化するheading text。]
//   戻り値: [String: 生成したslug。]
// }
fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for character in title.trim().to_lowercase().chars() {
        if character.is_alphanumeric() || character == '_' {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            pending_dash = false;
        } else if character.is_whitespace() || character == '-' {
            pending_dash = true;
        }
    }
    slug
}

// {
//   責務: [render_markdown_body: Markdown bodyの対応要素をegui widgetへ変換する。]
//   処理: [code fence・空行・table line・list・quote・inline textを識別し、code blockとinline rendererへ委譲する。]
//   引数: [ui: 描画先。 body: sectionのMarkdown本文。 current_document: relative linkの基準文書。 linked_target: 選択されたhelp linkの出力先。]
//   戻り値: [(): bodyを描画し、選択linkがあればtargetを記録する。]
// }
fn render_markdown_body(
    ui: &mut egui::Ui,
    body: &str,
    current_document: HelpDocument,
    linked_target: &mut Option<HelpTarget>,
) {
    let mut in_code_block = false;
    let mut code = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if in_code_block {
                render_code_block(ui, &code);
                code.clear();
            }
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            code.push_str(line);
            code.push('\n');
            continue;
        }
        if trimmed.is_empty() {
            ui.add_space(5.0);
        } else if trimmed.starts_with("| ") || trimmed.starts_with("|---") {
            if !trimmed.contains("---") || !trimmed.chars().all(|ch| "| :-".contains(ch)) {
                ui.label(egui::RichText::new(trimmed).monospace());
            }
        } else if let Some(item) = trimmed.strip_prefix("- ") {
            ui.horizontal_wrapped(|ui| {
                ui.label("•");
                render_inline(ui, item, current_document, linked_target);
            });
        } else if let Some(quote) = trimmed.strip_prefix("> ") {
            ui.horizontal_wrapped(|ui| {
                ui.weak("│");
                render_inline(ui, quote, current_document, linked_target);
            });
        } else {
            render_inline(ui, trimmed, current_document, linked_target);
        }
    }
    if in_code_block {
        render_code_block(ui, &code);
    }
}

// {
//   責務: [render_code_block: code textを横scroll可能なmonospace groupとして描画する。]
//   引数: [ui: 描画先。 code: 表示するfenced code本文。]
//   戻り値: [(): code block widgetを描画する。]
// }
fn render_code_block(ui: &mut egui::Ui, code: &str) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.label(egui::RichText::new(code).monospace());
        });
    });
}

// {
//   責務: [render_inline: inline textのMarkdown link表記を解析してlabelと通常textを描画する。]
//   処理: [link label・destinationを分割し、help文書linkをclick可能にして遷移targetを記録する。]
//   引数: [ui: 描画先。 text: inline Markdown text。 current_document: relative linkの基準文書。 linked_target: clickされた遷移先。]
//   戻り値: [(): linkと通常textを描画する。]
// }
fn render_inline(
    ui: &mut egui::Ui,
    text: &str,
    current_document: HelpDocument,
    linked_target: &mut Option<HelpTarget>,
) {
    ui.horizontal_wrapped(|ui| {
        let mut remaining = text;
        loop {
            let Some(open) = remaining.find('[') else {
                ui.label(clean_inline_markup(remaining));
                break;
            };
            let after_open = &remaining[open + 1..];
            let Some(label_end) = after_open.find("](") else {
                ui.label(clean_inline_markup(remaining));
                break;
            };
            let destination_start = open + 1 + label_end + 2;
            let Some(destination_end_relative) = remaining[destination_start..].find(')') else {
                ui.label(clean_inline_markup(remaining));
                break;
            };
            let destination_end = destination_start + destination_end_relative;
            let label = &after_open[..label_end];
            let destination = &remaining[destination_start..destination_end];
            if open > 0 {
                ui.label(clean_inline_markup(&remaining[..open]));
            }
            if is_help_document_link(destination) {
                if ui.link(clean_inline_markup(label)).clicked() {
                    *linked_target = resolve_link(current_document, destination);
                }
            } else {
                ui.label(clean_inline_markup(label));
            }
            remaining = &remaining[destination_end + 1..];
            if remaining.is_empty() {
                break;
            }
        }
    });
}

// {
//   責務: [is_help_document_link: destinationがviewer内のhelp文書へのlinkか判定する。]
//   処理: [fragmentを除いたpathを空pathまたはHelpDocumentのfile name一覧と照合する。]
//   引数: [destination: Markdown linkのdestination。]
//   戻り値: [bool: viewer内で解決できる文書linkならtrue。]
// }
fn is_help_document_link(destination: &str) -> bool {
    let path = destination
        .split_once('#')
        .map_or(destination, |(path, _)| path);
    path.is_empty()
        || HelpDocument::ALL
            .into_iter()
            .any(|document| document.file_name().eq_ignore_ascii_case(path))
}

// {
//   責務: [resolve_link: current document相対のMarkdown destinationをHelpTargetへ解決する。]
//   処理: [pathから文書を選び、fragmentまたは先頭sectionを既存anchorへ照合する。]
//   引数: [current_document: 空pathを解決する基準文書。 destination: 文書pathと任意fragment。]
//   戻り値: [Option<HelpTarget>: 有効な文書・anchorのtarget。文書またはanchorが不明ならNone。]
// }
fn resolve_link(current_document: HelpDocument, destination: &str) -> Option<HelpTarget> {
    let (path, requested_anchor) = destination
        .split_once('#')
        .map_or((destination, ""), |(path, anchor)| (path, anchor));
    let document = if path.is_empty() {
        current_document
    } else {
        HelpDocument::ALL
            .into_iter()
            .find(|document| document.file_name().eq_ignore_ascii_case(path))?
    };
    let page = HelpPage::new(document);
    let anchor = if requested_anchor.is_empty() {
        page.first_anchor()
    } else {
        page.anchor_for(requested_anchor)?
    };
    Some(HelpTarget::new(document, anchor))
}

// {
//   責務: [clean_inline_markup: 表示時に対応していないinline bold・code delimiterを除去する。]
//   引数: [text: 整形対象text。]
//   戻り値: [String: `**`とbacktickを取り除いた表示text。]
// }
fn clean_inline_markup(text: &str) -> String {
    text.replace("**", "").replace('`', "")
}
