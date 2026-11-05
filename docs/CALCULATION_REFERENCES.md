# Calculation参照モデルの設計契約

## 目的

Calculation BindingをrangeへFillしても意図した参照が維持されるよう、相対・絶対・混合参照を式ASTで表現します。

## 参照種類

表記はSpreadsheet慣習に合わせます。

```text
A1     row相対 / column相対
$A$1   row絶対 / column絶対
$A1    row相対 / column絶対
A$1    row絶対 / column相対
```

この表記はCalculation expression内だけの参照です。

CSVセル値として `=$A$1` 等を特別扱いしません。

## AST

概念:

```text
CellReference
├─ row index
├─ column index
├─ row mode: Relative | Absolute
└─ column mode: Relative | Absolute
```

## DSL上の表記と実装状況（#326）

DSLではセル参照を`CellValue("$A$1")` のように引用符内に書きます。`A1`・`$A$1`・`$A1`・`A$1` の4種類を受け付け、`$` は列文字の直前と行番号の直前だけに置けます（`A$$1`、`A1$`、`$1` は宣言時にerror）。generated.rlyへの保存でも同じ表記で往復します。

- 実装済み: 単一cellの4参照mode、DSL解析、評価時のresolve、serialize、range宣言のFill時に相対軸だけを移動する動作。`$`を含まない参照は従来どおり両軸相対です。
- 構造編集（#327、engine API）: `CalculationEngine::apply_structure_edit` が行・列の挿入削除に合わせてBindingのtargetと式内の全cell参照（相対・絶対とも）を同じ論理cellへ付け替え、全resultを再計算待ちにする。参照先が削除された参照は`DeletedReference`になり、評価は`DeletedReference` errorで、保存形式では`CellValue("#REF!")`と書く。targetの行・列が削除されたBindingは取り除く。`dsl:<cell>` 形式のIDは新しいtargetへ付け替える（DSL宣言とGUIがtargetからIDを導出するため）。それ以外のIDは変えない。付け替えた(旧ID, 新ID)は`StructureEditReport.renamed`で報告する。
- 未実装: 式内のrange参照とrange端点のmode（range参照を使う集計関数と同時に実装する）。GUIの行・列操作とUndo/Redoからengineへ構造編集を伝える接続（履歴が構造編集の種類と位置を公開する必要がある）。

## Fill

元target D2からD3へbindingをFillする場合:

```text
A1    -> A2
$A$1  -> $A$1
$A1   -> $A2
A$1   -> B$1   （横Fill時）
```

row/column deltaをrelative axisだけに適用します。

## Range

range両端にも同じ参照modeを持てます。

```text
$A$1:B10
```

## dependency

評価時は最終的にresolved CellRef / rangeへ変換しdependency indexへ登録します。

式のtext表現とresolved dependencyを混同しません。

## structure edit

row/column insert/delete時に絶対参照をどう追従するかは、単純index固定ではなくSpreadsheet参照の意味として定義します。

1.0では参照targetがinsert/deleteで移動した場合に同じlogical cellを追跡できるよう、structure edit側からreference rewriteを行います。

削除されたtargetはMissingCell等のerrorになります。

## Code / CSVとの分離

CodeモードはCSV raw textのみです。

Calculation参照syntaxをCode CSV parserへ導入しません。

## テスト

- 4種類の参照mode
- Fill down/right
- range Fill
- row/column insert追従
- deleted target error

この日本語版をCalculation参照モデルの正本とします。