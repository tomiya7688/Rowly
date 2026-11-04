# Group By・集計View・Pivotの設計契約

## 目的

表計算に必要な集計・クロス集計を、元CSVを書き換えないDerived Viewとして提供します。

## Group By View

例:

```text
Group by: department
Aggregate:
  SUM(sales)
  COUNT(*)
```

結果はDerived Tableです。

元CSVへ集計行を挿入しません。

## Aggregate

1.0で:

- COUNT
- COUNTA
- SUM
- AVERAGE
- MIN
- MAX
- DISTINCT COUNT

を提供します。

型変換規則はCalculation標準関数と共有します。

## Group key

raw値を既定とします。

明示型によるDate month/year grouping等はoptionとして追加できます。

表示formatが同じという理由だけでrawの異なる値を同groupへしません。

## Pivot

Pivotは:

```text
Rows
Columns
Values
Filters
```

からDerived Tableを作ります。

例:

```text
Rows: department
Columns: month
Values: SUM(sales)
```

## provenance

集計cellから元row集合へdrill downできるようにします。

```text
PivotCell
├─ aggregate value
└─ source row references
```

drill downのために元データを複製・別CSV化しません。

## stale / error

derived Calculation valueを集計sourceに使う場合、freshのみ通常集計へ含めます。

stale/error/pending件数をdiagnosticとして持ち、黙って0扱いしません。

## Filterとの順序

Pivot input scopeを明示します。

- all canonical rows
- current View filtered rows
- selection

current View scopeの場合、View Filter後・Pivot前です。

## Sort

Pivot結果の表示SortはDerived View上のSortです。

元CSV順を変更しません。

## 永続化

Pivot定義はView settingとしてProjectへ保存できます。

bare CSVではsession-only。

## export

Pivot結果をCSV/XLSXへ出す場合はDerived Table Exportとして明示します。

元CSV Saveとは別です。

## chart

ChartはPivot / aggregate resultをdata sourceにできます。

source freshnessを引き継ぎます。

## テスト

- Group Byで元CSV不変
- raw key grouping
- numeric aggregate
- distinct count
- pivot rows/columns
- drill down provenance
- View filtered scope
- stale derivedを0扱いしない

この日本語版をGroup By・Pivotの正本とします。