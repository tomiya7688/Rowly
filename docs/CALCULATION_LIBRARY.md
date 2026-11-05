# Calculation Binding 標準集計関数の設計契約

## 目的

表計算に必要な基本集計を、CSVセルへ数式文字列を埋め込まずCalculation Bindingとして提供します。

## 1.0必須関数

### 数値

- ABS
- ROUND
- FLOOR
- CEIL
- MOD

### 数値関数の規則

- ROUND: 引数1個。0から遠い方向へ四捨五入しIntegerを返す。
- FLOOR / CEIL: 引数1個。負/正の無限大方向へ丸めIntegerを返す。
- ROUND / FLOOR / CEILはDecimalの丸め結果がInteger範囲外ならoverflow errorにする。桁数指定は未対応。
- MOD: 引数2個。除数と同じ符号の剰余を返す。除数0は0除算error、型規則はInteger同士ならInteger、Decimalを含むならDecimal。

### 文字列

- LEN
- CONCAT
- LEFT
- RIGHT
- MID
- TRIM
- UPPER
- LOWER

### 日付

- DATE
- YEAR
- MONTH
- DAY

日付は明示的なDate解釈と組み合わせます。

### 集計

- SUM
- COUNT
- COUNTA
- AVERAGE
- MIN
- MAX

### 条件

- IF
- AND
- OR
- NOT

### 条件付き集計

- SUMIF
- COUNTIF

### lookup

- LOOKUP相当の単純exact lookup

高度な関数互換は1.0必須にしません。

## volatile関数

`TODAY()` / `NOW()` / random等は、同じ入力から同じ結果を得る再現性を壊します。

1.0の通常Calculation関数には含めません。

将来追加する場合は、暗黙のwall clockではなく明示的なVolatile Dependencyとして扱い、いつstaleになるか・履歴/再計算でどう扱うかを別仕様にします。

## 別source参照

Project内の別source / logical tableを参照する場合、表示tab名やpath文字列を恒久identityにしません。

将来のcross-source expressionはstable source_id / logical identityを明示する構文にします。

例（概念）:

~~~text
Source("sales").Cell("B2")
~~~

1.0でcross-source Calculationを実装しない場合も、ASTを現在source専用に固定して拡張不能にしません。

## Range

式はsingle cellだけでなくrange参照を値sourceとして扱えます。

例:

```text
SUM(B2:B100)
```

range自体はCSVへ保存される値ではありません。

## 型

Calculationは型付きです。

SUM/AVERAGE等はNumberへ明示変換可能な値だけを扱います。

Textを暗黙に0へ変換しません。

emptyの扱いも関数ごとに仕様化します。

## Error

例:

- MissingCell
- ExpectedNumber
- DivisionByZero
- EmptyAggregate
- LookupNotFound
- MultipleLookupMatches

error時もCSV raw valueは保持します。

## Freshness

range依存もdependency snapshotへ反映します。

range内の該当cellが変化したら結果をstale / recalculate対象にします。

大量rangeでcell全件snapshotが重い場合、range revision / digest等へ最適化できますが意味を変えません。

## IF

IFは通常DSLの任意副作用を実行しません。

pure Calculation AST内の条件分岐です。

```text
IF(A2 > 0, B2, C2)
```

## 条件付き集計

SUMIF/COUNTIFの条件はpure predicateです。

File I/Oやscript callを含めません。

## lookup

1.0ではexact matchを基本にします。

重複match時に先頭を暗黙採用せず、規則で明示するかerrorにします。

## 相対参照

range binding / fill bindingで相対参照を移動できます。

絶対参照を将来導入する場合は `$A$1` 等の表記をDSL/ASTで明示します。

## GUI

Calculation Editorで関数候補・引数help・参照rangeを選択できます。

数式バーへセル内数式として埋め込むUIにはしません。

## Materialize

関数結果のMaterializeはfresh resultだけ可能です。

Materialize後もbindingを残す/外すの既存契約に従います。

## テスト

- SUM/COUNT/AVERAGE/MIN/MAX
- empty / text混在
- IF
- SUMIF/COUNTIF
- exact lookup
- duplicate lookup
- range dependency freshness
- fill時相対参照

この日本語版をCalculation Binding標準集計関数の正本とします。