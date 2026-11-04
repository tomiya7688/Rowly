# 印刷・PDF・ページ出力の設計契約

## 目的

Viewで整えた表やChartを印刷・PDFへ出せるようにします。

印刷設定はCSV正本ではありません。

## 出力対象

- raw table
- current View
- named View
- Group By / Pivot result
- Chart

を明示的に選びます。

## Page Setup

1.0で:

- portrait / landscape
- paper size
- margins
- scale
- fit width
- header/footer
- repeat header row
- page break preview

を設計対象にします。

## CSVと分離

page breakやmarginのために空行をCSVへ挿入しません。

印刷用の列幅調整でCSV列を変更しません。

## PDF

PDFはrendered outputです。

CSV canonical Saveとは別Command:

```text
print.preview
print.execute
export.pdf
```

## Viewとの関係

current View出力ではFilter / Sort / column layout / formatting / groupingを反映できます。

visual groupingでunderlying dataを削除しません。

## stale data

freshでないderived resultが含まれる場合、既定ではPDF/print確定前に警告またはerrorにします。

古い計算値を正常帳票として黙って出力しません。

## print preview

previewは出力時のStateStampを持ちます。

データ変更後に古いpreviewをそのまま印刷しません。

## header/footer

page number、file/project名、View名等を表示できます。

CSV cellへ書き込みません。

## keyboard

Print Preview / Page Setup / Export PDFの全操作をAltから辿れます。

dialog内部もkeyboard-onlyで完結します。

## テスト

- print設定でCSV不変
- View filter/sort反映
- stale preview再利用拒否
- derived stale warning
- PDF exportでcanonical dirty不変

この日本語版を印刷・PDFの正本とします。