use std::error::Error;

use rowly::process::{
    CellRange, CellRef, CsvDocument, DocumentError, ValidationRule, ValidationTarget,
    calculation::{
        CalculationBindingId, CalculationEngine, CalculationExpression, CalculationTarget,
        CalculationTrigger,
    },
};
use tempfile::TempDir;

// {
//   責務: [create_document: clipboard検証用の保存済みCSVを一時directoryへ作る。],
//   処理: [行ごとの列数を保ったCSV fixtureを書き、process層で開く。],
//   引数: [rows: fixtureの各行とcell値。],
//   戻り値: [Result<(TempDir, CsvDocument), Box<dyn Error>>: fixture、または作成エラー。]
// }
fn create_document(rows: &[&[&str]]) -> Result<(TempDir, CsvDocument), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("clipboard.csv");
    let mut writer = csv::WriterBuilder::new().flexible(true).from_path(&path)?;
    for row in rows {
        writer.write_record(*row)?;
    }
    writer.flush()?;
    drop(writer);
    let document = CsvDocument::open(path)?;
    Ok((directory, document))
}

// {
//   責務: [snapshot_rows: CSVの値と各行の列数を比較するためのsnapshotを返す。],
//   処理: [documentの全行をテスト用に複製する。],
//   引数: [document: 比較対象のCSV document。],
//   戻り値: [Vec<Vec<String>>: 各行のcell値と行の長さを保持するsnapshot。]
// }
fn snapshot_rows(document: &CsvDocument) -> Vec<Vec<String>> {
    document.rows().map(<[String]>::to_vec).collect()
}

// {
//   責務: [copy_preserves_raw_value_and_paste_does_not_duplicate_binding: 計算結果とCSV値の分離を検証する。],
//   処理: [A2にderived resultを作り、A2のコピーをB2へ貼ってもCSV値だけが移ることを確認する。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、またはfixture・計算・編集エラー。]
// }
#[test]
fn copy_preserves_raw_value_and_paste_does_not_duplicate_binding() -> Result<(), Box<dyn Error>> {
    let (_directory, mut document) = create_document(&[&["target", "input"], &["raw", "9"]])?;
    let mut engine = CalculationEngine::default();
    let binding_id = CalculationBindingId::new("derived")?;
    engine.set_binding(
        &document,
        binding_id.clone(),
        CalculationTarget::Cell("A2".parse()?),
        CalculationExpression::Cell("B2".parse()?),
        CalculationTrigger::DependencyChange,
    )?;
    engine.recalculate_all(&document)?;
    assert_eq!(
        engine
            .result(&binding_id)
            .and_then(|result| result.value.as_deref()),
        Some("9")
    );

    let copied = document.copy_range_tsv("A2".parse()?)?;
    assert!(!document.is_dirty());
    assert!(!document.can_undo());
    document.paste_tsv_values("B2".parse()?, &copied)?;
    assert_eq!(document.cell_a1("B2")?, Some("raw"));
    assert_eq!(engine.bindings().count(), 1);
    assert_eq!(
        engine.binding(&binding_id).map(|binding| binding.target()),
        Some(CalculationTarget::Cell("A2".parse()?))
    );
    Ok(())
}

// {
//   責務: [quoted_values_and_ragged_rows_roundtrip_in_one_undo: TSV引用規則と短い行の形状保持を検証する。],
//   処理: [タブ・改行・引用符と欠けた末尾cellをコピーし、貼り付け全体を一度のUndo/Redoで往復する。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、またはfixture・編集エラー。]
// }
#[test]
fn quoted_values_and_ragged_rows_roundtrip_in_one_undo() -> Result<(), Box<dyn Error>> {
    let (_source_directory, source) = create_document(&[
        &["a", "b", "c"],
        &["=1+2", "a\tb", "日本語\n\"引用\""],
        &["short"],
    ])?;
    let (_target_directory, mut target) = create_document(&[
        &["a", "b", "c"],
        &["old-a", "old-b", "old-c"],
        &["old-short"],
    ])?;
    let original = snapshot_rows(&target);
    let copied = source.copy_range_tsv("A2:C3".parse()?)?;
    let changed_cells = target.paste_tsv_values("A2".parse()?, &copied)?;
    assert_eq!(
        changed_cells,
        vec!["A2".parse()?, "B2".parse()?, "C2".parse()?, "A3".parse()?]
    );
    assert_eq!(snapshot_rows(&target), snapshot_rows(&source));
    assert_eq!(target.rows().nth(2).map(<[String]>::len), Some(1));
    target.undo()?;
    assert_eq!(snapshot_rows(&target), original);
    assert!(!target.is_dirty());
    assert!(!target.can_undo());
    target.redo()?;
    assert_eq!(snapshot_rows(&target), snapshot_rows(&source));
    Ok(())
}

// {
//   責務: [validation_failure_preserves_values_and_redo: 検証失敗時に先行cellとredo履歴を保つことを検証する。],
//   処理: [有効値と無効値を同時に貼り、失敗後に元のredoを実行する。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、またはfixture・編集エラー。]
// }
#[test]
fn validation_failure_preserves_values_and_redo() -> Result<(), Box<dyn Error>> {
    let (_directory, mut document) = create_document(&[&["name", "state"], &["old", "ok"]])?;
    document.set_validation_rule(
        ValidationTarget::Index(1),
        ValidationRule::AllowedValues(vec!["ok".to_owned()]),
    )?;
    document.set_cell_a1("A2", "redo-value")?;
    document.undo()?;
    let original = document.csv_text()?;
    assert!(matches!(
        document.paste_tsv_values("A2".parse()?, "new\tinvalid\n"),
        Err(DocumentError::ValidationRejected { .. })
    ));
    assert_eq!(document.csv_text()?, original);
    assert!(!document.is_dirty());
    assert!(document.can_redo());
    document.redo()?;
    assert_eq!(document.cell_a1("A2")?, Some("redo-value"));
    Ok(())
}

// {
//   責務: [out_of_bounds_and_missing_cells_reject_the_entire_paste: 範囲外と短い行への非空値を拒否することを検証する。],
//   処理: [先行cellが有効でも、行列超過・座標overflow・欠損cellを含む貼り付けが全体を変更しないことを確認する。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、またはfixture作成エラー。]
// }
#[test]
fn out_of_bounds_and_missing_cells_reject_the_entire_paste() -> Result<(), Box<dyn Error>> {
    for (start, text) in [
        (CellRef::new(1, 0), "new\tvalue\textra\n"),
        (CellRef::new(1, 0), "new\tvalue\nother\tvalue\nextra\trow\n"),
        (CellRef::new(1, 0), "new\tvalue\nother\tmissing\n"),
        (CellRef::new(usize::MAX, 0), "overflow\n"),
        (CellRef::new(1, usize::MAX), "overflow\n"),
    ] {
        let (_directory, mut document) =
            create_document(&[&["a", "b"], &["old", "value"], &["short"]])?;
        let original = snapshot_rows(&document);
        assert!(document.paste_tsv_values(start, text).is_err());
        assert_eq!(snapshot_rows(&document), original);
        assert!(!document.is_dirty());
        assert!(!document.can_undo());
        assert!(!document.can_redo());
    }
    Ok(())
}

// {
//   責務: [header_paste_validates_rules_against_the_final_headers: header名の移動で無効になる値を拒否することを検証する。],
//   処理: [列名による検証規則を付け、header入れ替え後の列で検証する。無効な最終状態を拒否し、有効な最終状態を受理する。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、またはfixture・規則設定エラー。]
// }
#[test]
fn header_paste_validates_rules_against_the_final_headers() -> Result<(), Box<dyn Error>> {
    let (_directory, mut document) = create_document(&[&["state", "other"], &["ok", "invalid"]])?;
    document.set_validation_rule(
        ValidationTarget::Header("state".to_owned()),
        ValidationRule::AllowedValues(vec!["ok".to_owned()]),
    )?;
    let original = document.csv_text()?;
    assert!(matches!(
        document.paste_tsv_values("A1".parse()?, "other\tstate\n"),
        Err(DocumentError::ValidationRejected { .. })
    ));
    assert_eq!(document.csv_text()?, original);
    assert!(!document.is_dirty());
    document.paste_tsv_values("A1".parse()?, "other\tstate\ninvalid\tok\n")?;
    assert_eq!(document.cell_a1("B1")?, Some("state"));
    assert_eq!(document.cell_a1("B2")?, Some("ok"));
    document.undo()?;
    assert_eq!(document.csv_text()?, original);
    Ok(())
}

// {
//   責務: [unchanged_and_empty_pastes_preserve_redo: 変更なしの貼り付けが編集履歴を消さないことを検証する。],
//   処理: [Undo後に空文字列と同一値を貼り、redo可能な状態と保存済み状態を確認する。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、またはfixture・編集エラー。]
// }
#[test]
fn unchanged_and_empty_pastes_preserve_redo() -> Result<(), Box<dyn Error>> {
    let (_directory, mut document) = create_document(&[&["a", "b"], &["", "old"]])?;
    document.set_cell_a1("B2", "redo-value")?;
    document.undo()?;
    let copied = document.copy_range_tsv("A2:B2".parse()?)?;
    for text in ["", copied.as_str()] {
        assert!(document.paste_tsv_values("A2".parse()?, text)?.is_empty());
        assert!(!document.is_dirty());
        assert!(document.can_redo());
    }
    document.redo()?;
    assert_eq!(document.cell_a1("B2")?, Some("redo-value"));
    Ok(())
}

// {
//   責務: [copy_rejects_ranges_outside_the_existing_table: 不正なコピー範囲を空欄として成功扱いにしないことを検証する。],
//   処理: [最終行と最大列幅を超える範囲を指定し、原本と履歴を保ったままエラーになることを確認する。],
//   引数: [],
//   戻り値: [Result<(), Box<dyn Error>>: 全assertion成功、またはfixture作成エラー。]
// }
#[test]
fn copy_rejects_ranges_outside_the_existing_table() -> Result<(), Box<dyn Error>> {
    let (_directory, document) = create_document(&[&["only"]])?;
    for range in [
        "A1:A2".parse::<CellRange>()?,
        "A1:B1".parse()?,
        CellRange::new(CellRef::new(0, 0), CellRef::new(usize::MAX, usize::MAX)),
    ] {
        assert!(document.copy_range_tsv(range).is_err());
    }
    assert!(!document.is_dirty());
    assert!(!document.can_undo());
    Ok(())
}
