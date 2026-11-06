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

## Page Setupモデルとページ分割計算の実装契約（#307）

状態: 実装済み（`src/process/print_layout.rs`、`src/process/print_layout/setup.rs`）。確認日: 2026-11-06。

- `PageSetup`は向き・用紙（A3/A4/A5/B4/B5/Letter/Legal/任意mm）・余白・倍率（固定10〜400%または幅に合わせる）・見出し行の繰り返し・ページ順・ヘッダー/フッターを持つ。CSVの正本ではなくView / print stateで、CSVには何も書かない。
- `paginate(setup, table)`は純粋な計算で、`CsvDocument`を受け取らない。表の寸法（列幅mm・行高mm・見出し行数）はViewの表示設定から呼び出し側が渡す。ページ分割のためにCSVへ空行を足さず、印刷用の列幅もCSVへ書かない。
- 長さは整数のマイクロメートル、倍率はベーシスポイントで計算し、同じ入力なら常に同じページ数・同じ境界になる。
- 幅に合わせるは表の総幅が本文幅を超える場合だけ縮小し（下限10%）、拡大はしない。列は左から詰めて列方向に、行は上から詰めて行方向にページを分ける。
- 見出し行は2ページ目以降の先頭に繰り返し、その高さだけ本文の高さを減らす。見出し行がページ高さ以上なら繰り返しを取りやめ`header_repeat_skipped`で知らせる。
- 1ページに収まらない列・行は単独で1ページを占め、`oversized_columns` / `oversized_rows`に記録する（黙って切り捨てない）。行または列が無い表はページ0。
- ヘッダー/フッターは左・中央・右の文字列で、`{page}` `{pages}` `{file}` `{view}`を展開する。空でないときだけ本文の高さから8mmを取り分ける。
- 1つの長さ（用紙・余白・列幅・行高）は100,000mm以下、行数は1億以下に限り、超える値はoverflowさせずerrorにする。幅に合わせるは、列ごとの切り上げが累積して1ページ幅を溢れる場合に、列数分の余裕を見込んだ倍率へ下げ、最小倍率に達しない限り列を分けない。
- 対象外: 実際の描画・PDF出力、preview、StateStamp、stale警告、keyboard操作（印刷・PDF出力の後続Issue）。