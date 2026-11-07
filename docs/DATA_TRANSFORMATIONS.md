# 明示データ変換の設計契約

## 目的

Filter / Sort / formatは非破壊Viewですが、実務ではCSVそのものを整形・並べ替え・分割・重複除去したい場合があります。

その場合はView操作を暗黙にSaveするのではなく、**明示的なデータ変換Command**として正本CSVを変更します。

## 絶対条件

1. View SortだけではCSV順を変更しない。
2. View Filterだけでは行を削除しない。
3. データ変換は明示Command。
4. 実行前に変更計画を作る。
5. 1操作 = 1 Undo。
6. 複数sourceはOperationIdでatomic。

## 1.0基本変換

### View順を正本へ反映

`data.order.apply_view`

現在ViewのSort順をphysical CSV row orderへ反映します。

Filterで非表示の行をどう扱うかを明示します。

1.0既定:

- Filterは無視して全行を対象
- Sort keyだけをcanonical row orderへ反映

『表示中の行だけ残す』とはしません。

### Trim

- leading
- trailing
- both

対象range/columnへraw String変換として適用します。

### Case

- upper
- lower

locale依存の複雑なcase変換は仕様を明示します。

### Split Column

delimiter等で1列を複数列へ分割します。

出力列数とheaderをpreviewし、列追加をMutationPlanへ含めます。

### Combine Columns

複数列をseparator付きで1列へ結合します。

元列を残す/削除を明示します。

### Remove Duplicates

指定key列のraw値で重複判定します。

どのrowを残すか:

- first
- last

を明示します。

display formatだけが同じrowをduplicate扱いしません。

## Filter結果を削除

危険な操作なので別Commandです。

例:

```text
data.delete.filtered_rows
```

実行前に削除件数とsource別件数をpreviewします。

View Filterを解除しただけで元に戻る表示操作とは明確に分離します。

## Sort Materialize

logical tableが複数physical sourceを跨ぐ場合、View全体順をそのまま各CSVへ書けるとは限りません。

1.0では:

- single physical source: canonical reorder可能
- multi-source logical table: source内orderのみ / explicit redistribution command

を区別します。

source間row移動を暗黙に行いません。

## validation

値変換後の全cellをvalidationします。

Split/Combineでheaderやschemaが変わる場合、validation / View settings / Calculation Bindingのidentity再解決をChangeSetで通知します。

## preview

```text
変更cell: 120
削除row: 4
追加column: 2
source A: 80 changes
source B: 40 changes
```

等を表示できます。

StateStamp変更後の古いplanはcommitしません。

## CLI / DSL

GUI専用にしません。

同じTransformation CommandをRowly DSL / CLIから呼べるようにします。

## テスト

- View SortだけではCSV不変
- apply_view_orderで明示的にrow order変更
- Trim / Split / Combine
- Remove Duplicates raw比較
- validation failureでpartialなし
- multi-sourceを暗黙redistributeしない

この日本語版を明示データ変換の正本とします。

## Trim / Caseの実装契約（#313）

実装は `src/process/text_transform.rs` です。最終確認日: 2026-11-07。

- 対象は `TransformTarget::Range`（指定どおりのcell範囲。headerを含めれば変更する）か `Column`（データ行のみ。header行は列identityなので変更しない）です。
- Trimの空白はUnicodeのWhite_Space属性（`str::trim`）です。ゼロ幅スペースは空白として扱いません。
- Caseはロケールに依存しないUnicode既定変換です。`ß`は大文字で`SS`、語末の`Σ`は小文字で`ς`、`İ`は`i`と結合ドットになります。
- `plan_text_transform` が変更cell数と変更前後を返します。documentは変更しません。
- `apply_text_transform_plan` は、計画後にcellが変わっていれば拒否します（古い計画は確定しません）。
- 変換後の値が検証規則に反する場合は、全cellを変更せず `ValidationRejected` を返します。
- 変更は `CsvDocument::set_cell_values` で1件の履歴操作として記録するため、Undoは1回です。
- 変更が0件ならば履歴もdirtyも変えません。
- raw値だけを変更します。View設定・表示書式は変更しません。


## Remove Duplicatesの実装契約（#315）

実装は `src/process/remove_duplicates.rs` です。最終確認日: 2026-11-07。

- key列は1列以上を明示します。重複指定は1列として扱い、table外の列は拒否します。
- 比較はraw文字列の完全一致だけです。`1`と`1.0`、`A`と`a`、前後の空白違いは別の値です。表示書式・型解釈は使いません。
- 残す方針は `First`（最初の行）か `Last`（最後の行）です。残る行の順序は変えません。
- header行は比較にも削除にも含めません。
- `plan_remove_duplicates` が削除予定の行（各行と、同じkeyで残る行の対応つき）を返します。documentは変更しません。
- `apply_duplicate_plan` は、計画し直した結果が計画と一致しない場合（計画後にrowが変わった場合）は拒否します。削除は1つのtransactionにまとめ、Undoは1回です。transactionが既に開始されている場合は拒否します。
- 削除0件なら履歴もdirtyも変えません。
- 1つの `CsvDocument` は1つのphysical sourceです。複数sourceを跨ぐ重複除去は、source別または明示policyが必要なため扱いません。
- Calculation Bindingの追従は #343 の範囲です。
