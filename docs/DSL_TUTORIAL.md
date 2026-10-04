# Rowly DSL チュートリアル

この文書は、Rowly DSLを初めて使う人向けの日本語正本チュートリアルです。上から順に、短いCSV操作を実行しながら読み進めます。厳密な構文を調べるときは[文法リファレンス](DSL_REFERENCE.md)、変数の詳細は[DSL_BINDINGS](DSL_BINDINGS.md)、標準関数の詳細は[DSL_STANDARD_LIBRARY](DSL_STANDARD_LIBRARY.md)を参照してください。

<a id="tutorial-getting-started"></a>
## 始める前に

次のCSVを`lesson.csv`として保存し、Rowlyで開きます。CSVがデータの正本です。DSLの変数やclassは実行中だけ存在し、別のデータベースやsidecarを作りません。

```csv
名前,状態,得点,備考
田中,未着手,10,
佐藤,進行中,25,
Alice,完了,20,
```

コードブロックは現在実装済みの構文です。GUIにDSL入力欄がない場合は、Rust APIの`rowly_dsl::run(source, &mut document)`など、プロジェクトが提供するDSL実行入口へ渡します。CSVを保存する操作はDSL実行とは別に明示します。

<a id="tutorial-read-cell"></a>
## 1. セルを読む

`CellValue("A2")`は、実CSVのA1座標からStringを読みます。A1の行番号はヘッダー行を含みます。

```rowly
VAR name = CellValue("A2")
```

実行レポートの`name`は`田中`になります。空欄も空Stringとして読み取ります。

詳しくは[セル操作](DSL_REFERENCE.md#cells)を参照してください。

<a id="tutorial-write-cell"></a>
## 2. セルを書き換える

`SetCellValueAt`は1-basedの行・列を受け取ります。範囲編集は`This.Worksheet.Editor.Cell(...).Value.Set`で書けます。

```rowly
SetCellValueAt(Integer("2"), Integer("4"), "確認")
This.Worksheet.Editor.Cell(D3).Value.Set = "確認"
```

書き込みはprocessの編集履歴へ記録されます。保存したい場合は、DSL実行後にRowlyのSave操作を明示してください。

<a id="tutorial-bindings"></a>
## 3. VAR / CONSTを使う

`VAR`は再代入でき、`CONST`は名前の再代入を禁止します。数値計算をするときは明示変換します。

```rowly
VAR score = Integer(CellValue("C2"))
CONST bonus = Integer("5")
score = score + bonus
```

`LET` / `DIM`は使いません。同じscopeでの再宣言、未宣言名への代入、CONSTへの代入はエラーです。

<a id="tutorial-conditions"></a>
## 4. 条件分岐する

`If ... Then`、`Else`、`End If`で分岐します。Booleanを返す標準関数をそのまま条件に使えます。

```rowly
If Text.IsJapanese(CellValue("A2")) Then
    This.Worksheet.Editor.Cell(D2).Value.Set = "日本語あり"
Else
    This.Worksheet.Editor.Cell(D2).Value.Set = "日本語なし"
End If
```

`And` / `Or`は短絡評価され、`Not`が最も強く結び付きます。条件がBooleanでない場合は実行エラーです。

<a id="tutorial-for-loop"></a>
## 5. Forで全行を処理する

`For`は終了値を含む整数ループです。ヘッダーを除くデータ行を処理するときは2行目から始めます。

セル参照に`Drow`のような動的な名前は書けません。動的座標には`SetCellValueAt`を使います。

```rowly
For row = Integer("2") To RowCount()
    SetCellValueAt(row, Integer("4"), "確認")
Next row
```

ループ変数とループ内の宣言は反復ごとのscopeに置かれます。詳細は[For / Next](DSL_REFERENCE.md#for)を参照してください。

<a id="tutorial-headers"></a>
## 6. ヘッダー名で列を扱う

列番号が変わる可能性があるときは、ヘッダー名を使います。ヘッダーは先頭行と完全一致し、重複はエラーです。

```rowly
VAR status = CellValueByHeader(Integer("2"), "状態")
SetCellValueByHeader(Integer("2"), "備考", status)
VAR scoreColumn = ColumnIndex("得点")
```

`ColumnIndex`の戻り値は1-basedです。列名で読み書きするため、列の並びを変えてもスクリプトを保ちやすくなります。

<a id="tutorial-standard-functions"></a>
## 7. 標準関数で値を検査する

標準関数は`Text`、`Number`、`Boolean`の名前空間にあります。判定だけを行い、CSVの値を変換しません。

```rowly
VAR hasJapanese = Text.IsJapanese(CellValue("A2"))
VAR isScore = Number.IsInteger(CellValueByHeader(Integer("2"), "得点"))
VAR isState = Text.Contains(CellValueByHeader(Integer("2"), "状態"), "未")
```

Text系関数はStringを要求します。Number / Boolean系の不適合値はfalseになります。[標準関数](DSL_REFERENCE.md#standard-functions)も参照してください。

<a id="tutorial-functions"></a>
## 8. 関数を作る

`Def`で処理をまとめ、`Return`で値を返します。引数は呼び出しごとの可変local bindingです。

```rowly
Def AddBonus(value)
    Return value + Integer("5")
End Def
VAR result = AddBonus(Integer(CellValue("C2")))
```

関数内の変数は呼び出しが終わると破棄されます。式に使う関数は値をReturnしてください。

<a id="tutorial-transactions"></a>
## 9. transactionで一括編集する

複数の編集を一つのUndo単位にまとめるときは、明示的にtransactionを開始・確定します。

```rowly
BeginTransaction()
SetCellValueByHeader(Integer("2"), "状態", "進行中")
SetCellValueByHeader(Integer("2"), "備考", "確認待ち")
CommitTransaction()
```

途中で取り消す場合は`RollbackTransaction()`を使います。transactionのネスト、activeでないtransactionのcommit / rollbackはエラーです。SaveやUndoの可否はprocess層の規則に従います。

<a id="tutorial-classes"></a>
## 10. classとmethodで処理を整理する

classは実行中の値と処理をまとめます。`Self`は現在のinstanceです。

```rowly
Class Label
    Field text = ""
    Def Init(value)
        Self.text = value
    End Def
    Def Write(row)
        SetCellValueByHeader(row, "備考", Self.text)
    End Def
End Class
VAR label = New Label("確認済み")
label.Write(Integer("2"))
```

`CONST label`としても、bindingの再代入だけが禁止され、フィールド更新は可能です。classの詳細は[Class / Field / Method](DSL_REFERENCE.md#classes)を参照してください。

<a id="tutorial-example"></a>
## 11. 実用例を完成させる

最後に、得点が10以上の行へ状態と備考を書き込みます。列番号へ依存せず、transactionと関数を組み合わせます。

```rowly
Def Mark(row)
    VAR score = Integer(CellValueByHeader(row, "得点"))
    If score >= Integer("10") Then
        SetCellValueByHeader(row, "状態", "確認済み")
        SetCellValueByHeader(row, "備考", "自動判定")
    End If
End Def

BeginTransaction()
For row = Integer("2") To RowCount()
    Mark(row)
Next row
CommitTransaction()
```

実行後は表を確認し、必要ならRowlyのSaveでCSVへ保存します。DSLはCSVの正本を置き換えず、processの編集APIを通じて変更と履歴を作ります。詳細なエラー条件や制限は[文法リファレンス](DSL_REFERENCE.md)に戻って確認してください。

<a id="tutorial-troubleshooting"></a>
## エラーが出たとき

まずエラーの行番号と構文を確認します。`CellValueByHeader`のheaderは完全一致、座標は1-based、`Text`系の引数はStringです。構文を調べるときは[文法リファレンス](DSL_REFERENCE.md)、宣言やscopeの挙動は[DSL_BINDINGS](DSL_BINDINGS.md)を参照してください。

DSL実行のエラーは、それ以前に成功した通常編集を自動で巻き戻すものではありません。まとめて安全に戻したい処理はtransactionを使い、呼び出し側で状態を確認してrollbackしてください。
