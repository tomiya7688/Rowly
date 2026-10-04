// {
//   責務: [
//     main: native eframe windowを起動してRowly GUIを表示する
//   ]
//   処理: [
//     1: defaultのnative window optionsを作成する
//     2: RowlyAppを生成してeframe event loopを開始する
//   ]
//   引数: []
//   戻り値: [
//     eframe::Result: GUI起動またはevent loopの結果
//   ]
//   副作用: [
//     native windowとGUI event loopを開始する
//   ]
// }
fn main() -> eframe::Result {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Rowly",
        options,
        Box::new(|_creation_context| Ok(Box::new(rowly::gui::RowlyApp::default()))),
    )
}
