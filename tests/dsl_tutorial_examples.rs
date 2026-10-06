use std::fs;

use rowly::{process::CsvDocument, rowly_dsl};

#[test]
// {
//   責務: [tutorial_examples_parse_and_execute_independently: DSL tutorial内の各rowly code blockが独立fixture上で実行できることを確認する。]
//   処理: [Markdown fenceからlesson例を抽出し、transaction cleanupと最低lesson数を検証する。]
//   戻り値: [(): assertion成功時は値を返さない。]
//   副作用: [各exampleごとに一時directoryとCSV fixtureを作成する。]
//   エラー: [fence形式・DSL実行・open transaction・example数が契約を満たさない場合はtestを失敗させる。]
// }
fn tutorial_examples_parse_and_execute_independently() {
    let tutorial = include_str!("../docs/DSL_TUTORIAL.md").replace("\r\n", "\n");
    let fixture = "名前,状態,得点,備考\n田中,未着手,10,\n佐藤,進行中,25,\nAlice,完了,20,\n";
    let mut source = None;
    let mut examples = 0;
    for (index, line) in tutorial.lines().enumerate() {
        if line == "```rowly" {
            assert!(source.is_none(), "nested example at line {}", index + 1);
            source = Some((index + 1, String::new()));
        } else if line == "```" {
            if let Some((start, script)) = source.take() {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("tutorial.csv");
                fs::write(&path, fixture).unwrap();
                let mut document = CsvDocument::open(&path).unwrap();
                rowly_dsl::run(&script, &mut document)
                    .unwrap_or_else(|error| panic!("tutorial example at line {start}: {error}"));
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
    assert!(source.is_none(), "unterminated tutorial example");
    assert!(
        examples >= 11,
        "tutorial must retain one example per lesson"
    );
}
