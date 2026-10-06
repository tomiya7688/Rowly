use std::fs;

use rowly::{process::CsvDocument, rowly_dsl};

#[test]
// {
//   責務: [japanese_reference_examples_parse_and_execute_independently: DSL reference内の各rowly code blockが独立したCSV document上で実行できることを確認する。]
//   処理: [Markdown fenceからCSV fixtureとDSL例を抽出し、transaction cleanupと必要な例数を検証する。]
//   戻り値: [(): assertion成功時は値を返さない。]
//   副作用: [各exampleごとに一時directoryとCSV fixtureを作成する。]
//   エラー: [fence形式・DSL実行・open transaction・example数が契約を満たさない場合はtestを失敗させる。]
// }
fn japanese_reference_examples_parse_and_execute_independently() {
    let reference = include_str!("../docs/DSL_REFERENCE.md").replace("\r\n", "\n");
    let fixture = reference
        .split_once("```csv\n")
        .and_then(|(_, rest)| rest.split_once("\n```"))
        .map(|(csv, _)| format!("{csv}\n"))
        .expect("reference must define the CSV used by its examples");
    let mut source = None;
    let mut examples = 0;
    for (index, line) in reference.lines().enumerate() {
        if line == "```rowly" {
            assert!(source.is_none(), "nested example at line {}", index + 1);
            source = Some((index + 1, String::new()));
        } else if line == "```" {
            if let Some((start, script)) = source.take() {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("reference.csv");
                fs::write(&path, &fixture).unwrap();
                let mut document = CsvDocument::open(&path).unwrap();
                rowly_dsl::run(&script, &mut document)
                    .unwrap_or_else(|error| panic!("reference example at line {start}: {error}"));
                assert!(
                    !document.transaction_active(),
                    "open transaction at line {start}"
                );
                examples += 1;
            }
        } else if let Some((_, script)) = &mut source {
            script.push_str(line);
            script.push('\n');
        }
    }
    assert!(source.is_none(), "unterminated reference example");
    assert!(
        examples >= 18,
        "reference must retain examples for the documented topics"
    );
}
