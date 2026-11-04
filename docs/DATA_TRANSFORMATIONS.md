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