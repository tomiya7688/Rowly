use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

// ```text
// 責務: [install_japanese_font: 同梱フォントを通常文字と等幅文字の代替に登録する]
// 処理: [既定の文字を優先し、日本語など既定フォントにない文字を同梱フォントで表示する]
// 引数: [context: GUI の描画設定]
// 戻り値: [(): フォント設定を反映する]
// ```
pub fn install_japanese_font(context: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "noto-sans-jp".to_owned(),
        FontData::from_static(include_bytes!(
            "../../assets/fonts/NotoSansCJKjp-Regular.otf"
        ))
        .into(),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        // 既定フォントのラテン文字と記号を維持し、日本語をフォールバックする。
        fonts
            .families
            .entry(family)
            .or_default()
            .push("noto-sans-jp".to_owned());
    }
    context.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // ```text
    // 責務: [japanese_glyphs_are_available_in_both_families: 表示と CSV 編集の両方で日本語が欠けないことを検証する]
    // 戻り値: [(): 日本語と ASCII の文字を両フォント系統が持つ場合に成功する]
    // ```
    fn japanese_glyphs_are_available_in_both_families() {
        let context = egui::Context::default();
        install_japanese_font(&context);
        let _output = context.run(egui::RawInput::default(), |context| {
            context.fonts(|fonts| {
                for family in [FontFamily::Proportional, FontFamily::Monospace] {
                    let font = egui::FontId::new(16.0, family);
                    for character in "日本語あいうカナABC".chars() {
                        assert!(
                            fonts.has_glyph(&font, character),
                            "missing {character} in {font:?}"
                        );
                    }
                }
            });
        });
    }
}
