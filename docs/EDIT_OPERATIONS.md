# 一括入力・Fill・Paste Specialの設計契約

## 目的

表計算で頻繁に使う連続入力・複製・転置貼り付けを、CSV正本とCalculation Bindingの分離を壊さず提供します。

## 操作を分ける

Rowlyでは『値を複製する』と『計算規則を複製する』を別Commandにします。

```text
edit.fill.values
calculation.fill.bindings
edit.series.generate
edit.paste.values
edit.paste.transpose
```

Excelのように1つのFill操作が値と数式を暗黙に混ぜません。

## Fill Values

選択元のcanonical raw valueを対象rangeへ複製します。

例:

```text
A2 = 東京
A3:A10へFill Down
```

結果はA3:A10へ文字列 `東京` を書き込みます。

### pattern

複数元cellを指定した場合:

```text
1
2
1
2
...
```

のような繰り返しpatternを既定にします。

数値系列へ勝手に推測しません。

## Series Generate

連続数・日付等を生成する操作はFill Valuesと分けます。

例:

```text
開始: 1
step: 1
count: 100
```

生成値を事前previewしてからcanonical CSVへ1操作で書き込みます。

### 1.0 series

- Integer
- Decimal
- Date（明示されたformat / parser）

文字列から暗黙に『日付っぽい』と推測して系列化しません。

## Calculation Binding Fill

Calculation Bindingの複製では、target位置に応じて相対参照を移動します。

```text
D2 = B2 * C2
↓ fill down
D3 = B3 * C3
D4 = B4 * C4
```

CSV raw valueは変更しません。

Bindingのfillとraw value fillを1Commandへ統合しません。

## Paste Special

通常Pasteはraw CSV valuesのみです。

追加するPaste Special:

- values
- transpose values
- repeat values into selection

Calculation BindingやView formatは通常Pasteで複製しません。

Bindingを貼り付ける場合は専用Commandにします。

## Transpose

```text
A1:B2
a,b
c,d

↓ transpose

a,c
b,d
```

transposeは貼り付け先範囲を事前に計算し、全件validation後にatomic commitします。

## 選択範囲を超える場合

既存CSVの外へ自動的に無限sheetを拡張しません。

必要なら行/列追加を明示的にplanへ含め、利用者へ結果を提示します。

1.0では既存table bounds外のPaste/Fillを拒否しても構いません。

## 入力規則

Fill / Series / Paste Specialは全対象を先にvalidationします。

1件でも違反すれば既定では全件適用しません。

## Undo

Fill / Series / Paste Specialそれぞれ1操作 = 1 Undoです。

複数sourceを跨ぐ場合はOperationIdで1 Undo groupへ束ねます。

## Preview

Seriesや大量Fillでは変更件数と先頭例をpreviewできます。

古いStateStampのpreviewをそのままcommitしません。

## コマンド

```text
edit.fill.values.down
edit.fill.values.right
edit.series.generate
edit.paste.values
edit.paste.transpose
calculation.fill.bindings.down
calculation.fill.bindings.right
```

全GUI操作はAltから到達可能にします。

## テスト

- raw FillでCalculation Bindingを複製しない
- Binding FillでCSV raw valueを変更しない
- pattern repeat
- explicit series
- transpose
- validation failureで部分適用なし
- 1 Undoで全体復元

この日本語版をFill / Series / Paste Specialの正本とします。

## Fill Values / Series Generateの実装契約（#279）

実装は `src/process/fill.rs` です。最終確認日: 2026-11-08。

- `plan_fill_values(範囲, 方向, pattern_len)` は、範囲の上端（Down）または左端（Right）の `pattern_len` 個を元cellとし、残りへraw文字列を繰り返し複製します。複数の元cellは繰り返しpatternで、数値系列へ推測しません。
- `plan_series(範囲, 方向, generator)` は単一の列（Down）または行（Right）へ、明示した `SeriesGenerator` の値を先頭から順に割り当てます。長さは範囲の長さです。
- 組み込みのgeneratorは `IntegerSeries`（小数点なし）と `DecimalSeries` です。小数は浮動小数点を使わず10進の整数演算で計算し、開始値と増分の大きい方の桁数で出力します（開始1・増分0.5なら1.0, 1.5, 2.0）。指数表記・空白・空の整数部/小数部・30桁超・overflowは拒否します。
- Date seriesは `SeriesGenerator` を実装する明示format/parserで追加します。文字列から日付を推測しません。
- 範囲が既存table外に出る場合は拒否します（tableを自動拡張しません）。
- `apply_fill_plan` は、元cellと対象cellが計画時から変わっていれば拒否します。値が変わるcellだけを `set_cell_values` で全件検証して書き込み、違反が1件でもあれば何も変更しません。Undoは1回で、変更0件なら履歴もdirtyも変えません。
- Calculation Bindingは複製も変更もしません。GUI・Paste Specialは対象外です。


## Paste Specialの実装契約（#281）

実装は `src/process/paste_special.rs` です。最終確認日: 2026-11-08。

- 貼り付ける値は `ClipboardGrid`（長方形のraw文字列）です。TSVからは `ClipboardGrid::from_tsv` で作り、引用されたタブ・改行はraw値として保持します。
- 方式は `PasteMode::Values`（選択範囲の左上から貼る）、`Transpose`（行と列を入れ替えて左上から貼る。2x3は3x2になる）、`Repeat`（選択範囲全体へタイル状に繰り返す）です。
- 行ごとに列数が違う（ragged）clipboardは `RaggedPolicy::Reject` が既定で拒否します。`PadEmpty` を明示した場合だけ、短い行の右を空文字で埋めます。空のclipboardは拒否します。
- `Repeat` の選択範囲はclipboardの大きさの整数倍に限ります。端数のtileは黙って切り捨てず拒否します。
- 書込み先が既存table外に出る場合は拒否します（行・列を自動で追加しません）。
- 計画は `FillPlan` で、適用は `apply_fill_plan` です。対象cellが計画時から変わっていれば拒否し、全件を検証してから1回のUndoで書き込みます。違反が1件でもあれば何も変更せず、変更0件なら履歴もdirtyも変えません。
- raw文字列だけを貼り付け、Calculation BindingとView styleは複製しません。
