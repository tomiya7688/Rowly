use std::fs;

use rowly::{process::CsvDocument, rowly_dsl};

#[test]
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
