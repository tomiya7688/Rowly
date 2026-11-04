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