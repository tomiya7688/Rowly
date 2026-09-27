fn main() -> eframe::Result {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Rowly",
        options,
        Box::new(|_creation_context| Ok(Box::new(rowly::gui::RowlyApp::default()))),
    )
}
