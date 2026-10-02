use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpDocument {
    Reference,
    Tutorial,
    Bindings,
    StandardLibrary,
}

impl HelpDocument {
    const ALL: [Self; 4] = [
        Self::Reference,
        Self::Tutorial,
        Self::Bindings,
        Self::StandardLibrary,
    ];

    const PRIMARY: [Self; 2] = [Self::Reference, Self::Tutorial];

    fn title(self) -> &'static str {
        match self {
            Self::Reference => "DSL 文法リファレンス",
            Self::Tutorial => "DSL チュートリアル",
            Self::Bindings => "DSL の変数・定数・再代入",
            Self::StandardLibrary => "DSL 標準関数",
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Self::Reference => "DSL_REFERENCE.md",
            Self::Tutorial => "DSL_TUTORIAL.md",
            Self::Bindings => "DSL_BINDINGS.md",
            Self::StandardLibrary => "DSL_STANDARD_LIBRARY.md",
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::Reference => include_str!("../docs/DSL_REFERENCE.md"),
            Self::Tutorial => include_str!("../docs/DSL_TUTORIAL.md"),
            Self::Bindings => include_str!("../docs/DSL_BINDINGS.md"),
            Self::StandardLibrary => include_str!("../docs/DSL_STANDARD_LIBRARY.md"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelpTarget {
    pub document: HelpDocument,
    pub anchor: String,
}

impl HelpTarget {
    pub fn new(document: HelpDocument, anchor: impl Into<String>) -> Self {
        Self {
            document,
            anchor: anchor.into(),
        }
    }
}

#[derive(Debug)]
struct HelpSection {
    anchor: String,
    title: String,
    level: usize,
    body: String,
}

#[derive(Debug)]
struct HelpPage {
    document: HelpDocument,
    sections: Vec<HelpSection>,
}

impl HelpPage {
    fn new(document: HelpDocument) -> Self {
        Self {
            document,
            sections: parse_sections(document.source()),
        }
    }

    fn first_anchor(&self) -> &str {
        self.sections
            .first()
            .map_or("", |section| section.anchor.as_str())
    }

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

#[derive(Debug)]
pub struct HelpViewer {
    pages: Vec<HelpPage>,
    active_document: HelpDocument,
    selected_anchor: String,
    heading_filter: String,
    pending_scroll: Option<String>,
}

impl Default for HelpViewer {
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

    fn page(&self, document: HelpDocument) -> &HelpPage {
        self.pages
            .iter()
            .find(|page| page.document == document)
            .expect("all help documents are loaded")
    }
}

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

fn parse_explicit_anchor(line: &str) -> Option<&str> {
    line.strip_prefix("<a id=\"")?.strip_suffix("\"></a>")
}

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

fn render_code_block(ui: &mut egui::Ui, code: &str) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.label(egui::RichText::new(code).monospace());
        });
    });
}

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

fn is_help_document_link(destination: &str) -> bool {
    let path = destination
        .split_once('#')
        .map_or(destination, |(path, _)| path);
    path.is_empty()
        || HelpDocument::ALL
            .into_iter()
            .any(|document| document.file_name().eq_ignore_ascii_case(path))
}

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

fn clean_inline_markup(text: &str) -> String {
    text.replace("**", "").replace('`', "")
}
