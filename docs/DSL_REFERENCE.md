# Rowly DSL 文法リファレンス

この文書は、現在実装済みの Rowly DSL 1.0 構文を調べるための**日本語正本リファレンス**です。全構文の適合確認・仕様凍結は別途 #55 で扱います。ここでは実装済みの意味を記載し、未実装の将来構文を混在させません。

宣言・束縛の詳細は [DSL_BINDINGS.md](DSL_BINDINGS.md)、標準判定関数の詳細は [DSL_STANDARD_LIBRARY.md](DSL_STANDARD_LIBRARY.md) を正本とします。本書はそれらへの索引と、その他の実装済み構文の参照先です。

## 索引

- [行・コメント・識別子](#source)、[リテラルと値型](#values)、[明示変換](#conversions)
- [VAR / CONST と再代入](#bindings)、[スコープ](#scope)
- [演算子と優先順位](#operators)、[比較・論理条件](#conditions)
- [If / Else](#if)、[For / Next](#for)
- [関数定義・呼び出し](#functions)、[Return](#return)
- [Class / Field / Method / New](#classes)、[Self / Init](#self-init)、[継承 / Super](#inheritance)
- [標準名前空間](#standard-functions)
- [A1セル・範囲](#cells)、[行列数・動的座標](#dimensions)、[ヘッダー名](#headers)、[列チェック](#column-checks)、[入力規則](#validation-rules)
- [transaction](#transactions)、[CSV書き込みと制限](#limits)、[未実装構文との境界](#unsupported)

明示したアンカーIDはアプリ内Helpからの参照用です。見出しの文言変更時もIDを維持してください。

本書の `rowly` コードブロックは、次のCSVを開いた状態で、それぞれ独立に実行できる最小例です。CSVが正本であり、DSL変数やオブジェクトは実行中だけの値です。コード実行のRust入口は `rowly_dsl::run(source, &mut document)` です。CLIにDSL実行コマンドがあることを前提にしません。

```csv
名前,状態,得点,備考
田中,未着手,10,
Alice,完了,20,
```

<a id="source"></a>
## 行・コメント・識別子

原則1行に1文を書き、ブロックの開始と終了も別行に置きます。空行と前後の空白は読み飛ばします。行頭の `'`、`Rem` 単独、`Rem ` で始まる行はコメントです。一般的な行継続、行末コメント、セミコロンによる複数文は提供しません。入力規則のAllowedValuesリストと未完了のExpression論理式のみ、次の行へ続けて記述できます。

```rowly
' 名前は実行ごとの変数
Rem 大小文字を区別しない英字キーワード
VAR 名前 = "田中"
```

キーワード・識別子の英字部分はASCIIの大小文字を区別しません。識別子は先頭が文字または `_`、後続が文字・数字・`_` で、日本語も使えます。変数・引数・ループ変数には `Self` / `Super` / `true` / `false` / `VAR` / `CONST` / `LET` / `DIM` / `Text` / `Number` / `Boolean` を使えません。構文中の空白は以下の例に合わせてください。任意の場所に空白や改行を挿入できる文法ではありません。

主なエラー: 不正・予約済みbinding名、閉じていないブロック、対応しない終了文。全体を解析してから実行するため、実行しない分岐や関数の構文エラーも検出します。

<a id="values"></a>
## リテラルと値型

| 値型 | 作り方 | 性質 |
| --- | --- | --- |
| String | `"001"`、`""`、未引用の `8` / `true` / `false` | リテラルとCSV読み取りは文字列 |
| Integer | `Integer("8")`、行列数・列番号の戻り値 | 符号付き64-bit整数 |
| Decimal | `Decimal("1.5")`、除算 | `f64`、変換・二項算術結果は有限値を要求 |
| Boolean | `Boolean("true")`、標準判定関数 | 型付きの真偽値 |
| Object | `New ClassName(...)` | 実行中のオブジェクト参照 |

```rowly
CONST code = "001"
VAR bareNumber = 8
VAR bareBoolean = true
VAR count = Integer("8")
VAR enabled = Boolean("true")
VAR message = "1行目\n2行目\t\"引用\"\\"
```

`8` と `true` はInteger / Booleanリテラルではありません。未引用の識別子は変数参照なので、通常の文字列は二重引用符で囲みます。文字列内は `\n` / `\t` / `\"` / `\\` を解釈します。その他の `\x` は現在のparserではbackslashを除いた `x` になります。CSVの引用符エスケープ `""` とDSLの `\"` は別の規則です。

主なエラー: 閉じていない引用符、引用符終了後の余分な文字、未知の変数。文字列自体をソース上で複数行にせず、改行は `\n` と書きます。

<a id="conversions"></a>
## 明示変換

| 構文（各1引数） | 受け取れる値 | 最小例 |
| --- | --- | --- |
| `Integer(value)` | Integer、`i64`として解釈できるString | `Integer("10")` |
| `Decimal(value)` | Decimal、Integer、有限`f64`として解釈できるString | `Decimal("1.5")` |
| `Boolean(value)` | Boolean、ASCII大小文字を無視した`true` / `false`のString | `Boolean("TRUE")` |
| `String(value)` | Object以外の値 | `String(Integer("10"))` |

```rowly
VAR score = Integer(CellValue("C2"))
VAR half = Decimal(score) / Integer("2")
VAR output = String(half)
```

暗黙の文字列→数値変換は行いません。`Integer(Decimal(...))` による丸めや `Boolean(Integer(...))` による真偽判定も提供しません。

主なエラー: 引数個数の不一致、変換元の型・文字列が不適合、整数範囲外、非有限Decimal、Objectの文字列化。

<a id="bindings"></a>
## VAR / CONST と再代入

構文: `VAR name = expression`、`CONST name = expression`、`name = expression`。宣言には初期値が必須です。VARは再代入可能、CONSTは名前の再束縛を禁止します。

```rowly
VAR count = Integer("0")
CONST increment = Integer("2")
count = count + increment
```

主なエラー: 未宣言名への代入、CONSTへの代入、同じスコープでの再宣言、初期値なし、旧 `LET` / `DIM`。存在・可変性・再宣言の検証は右辺評価より先なので、拒否した代入の右辺にあるCSV編集関数を実行しません。詳細は [宣言と再代入](DSL_BINDINGS.md#宣言と再代入)、[エラーと副作用](DSL_BINDINGS.md#エラーと副作用) を参照してください。

<a id="scope"></a>
## スコープ

| 場所 | bindingの寿命・参照 |
| --- | --- |
| トップレベル | そのDSL実行中のglobal scope |
| 関数・メソッド・Init | 呼び出しごとのlocal scope。引数は可変binding |
| If / Else | 新しいscopeを作らず、実行した分岐の宣言を共有 |
| For / Next | 反復ごとのscope。ループ変数・反復内宣言は反復終了で破棄 |

参照と再代入は呼び出し時に見えている最も内側のbindingから探します。外側のVARへ明示的に代入でき、内側で同名を宣言すれば外側を隠します。クロージャや静的な定義位置だけによる解決ではありません。Returnや実行エラーでも呼び出しscopeを取り除きます。

```rowly
VAR total = Integer("0")
For row = Integer("2") To RowCount()
    CONST score = Integer(CellValueByHeader(row, "得点"))
    total = total + score
Next row
```

主なエラー: 破棄済みlocalの参照、引数と同名の再宣言、外側CONSTへの代入。詳細は [スコープ](DSL_BINDINGS.md#スコープ) を参照してください。

<a id="operators"></a>
## 演算子と優先順位

| 強い順 | 構文 | 意味 |
| --- | --- | --- |
| 1 | `(expression)`、呼び出し・member参照 | 括弧内を先に評価 |
| 2 | `-expression` | 型付き数値の符号反転 |
| 3 | `left * right`、`left / right` | 乗算・除算 |
| 4 | `left + right`、`left - right` | 加算・減算 |

同じ優先順位の二項算術は左結合です。Integer同士の `+` / `-` / `*` はInteger、Decimalを含む組み合わせはDecimal、`/` はInteger同士でもDecimalです。`+` による文字列連結や単項 `+` は提供しません。

```rowly
VAR result = Integer("2") + Integer("3") * Integer("4")
VAR grouped = (Integer("2") + Integer("3")) * Integer("4")
VAR negative = -Integer("2")
VAR quotient = Integer("5") / Integer("2")
```

主なエラー: String / Boolean / Objectへの算術、ゼロ除算、Integerのoverflow、二項算術の非有限結果。

<a id="conditions"></a>
## 比較・論理条件

条件は `If ... Then` に書きます。比較演算子は `=` / `!=` / `<` / `<=` / `>` / `>=`。論理結合は強い順に `Not`、`And`、`Or` で、括弧で変更できます。`And` は左がfalseなら、`Or` は左がtrueなら右を評価しません。比較・論理結合は現在の一般値式ではなく条件構文です。

```rowly
If Integer(CellValue("C2")) >= Integer("10") And Not Boolean("false") Then
    SetCellValueAt(Integer("2"), Integer("4"), "対象")
End If
```

String同士は文字列として比較、Integer / Decimalは数値として比較します。`=` / `!=` はBoolean同士やObjectの同一性にも使え、異種型の等価比較はfalseです（Integer / Decimalの組み合わせは数値比較）。Boolean / Objectの大小比較やStringと数値の大小比較はエラーです。`==` / `<>` を比較構文として使用しません。

比較なしの条件にはBooleanを返す式が必要です。未引用 `true` もStringなので、`If Boolean("true") Then` と書きます。比較の連鎖は使わず、`a < b And b < c` のように条件を分けます。

<a id="if"></a>
## If / Else / End If

構文は `If condition Then`、任意の `Else`、`End If`。ネストできます。`EndIf` も終了表記として受理します。

```rowly
If Text.IsJapanese(CellValue("A2")) Then
    This.Worksheet.Editor.Cell(D2).Value.Set = "日本語あり"
Else
    This.Worksheet.Editor.Cell(D2).Value.Set = "日本語なし"
End If
```

主なエラー: `Then` / `End If` の欠落、Elseの重複、対応しない終了文、非Boolean条件。`Else If` の専用構文はなく、Else内に別のIfをネストします。

<a id="for"></a>
## For / To / Step / Next

構文: `For name = start To end [Step step]` ... `Next [name]`。開始・終了・StepはInteger値で、開始前に1回評価します。終了値を含み、Step省略時は1、負なら降順です。開始時点で範囲外なら0回です。

```rowly
For row = RowCount() To Integer("2") Step -Integer("1")
    SetCellValueByHeader(row, "備考", "確認済み")
Next row
```

ループ変数への再代入はその反復のbindingだけを変更し、次の反復を決める内部カウンタは変更しません。反復ごとのscopeは[スコープ](#scope)を参照してください。

主なエラー: 非Integerの境界・Step、Step=0、内部カウンタのoverflow、`Next` の名前不一致、`Next` 欠落。

<a id="functions"></a>
## 関数定義・呼び出し

構文: トップレベルの `Def name(parameter, ...)` ... `End Def`。終了表記 `EndDef` も受理します。呼び出しは `name(argument, ...)` で、引数は左から順に評価します。定義は全体の解析時に収集され、記述位置より前から呼び出せます。

```rowly
Def Double(value)
    Return value * Integer("2")
End Def
VAR answer = Double(Integer("10"))
```

関数を単独の文として呼ぶと戻り値を捨てます。式中の呼び出しは戻り値が必要です。組み込み関数は同名のユーザー関数より先に解決します。標準名前空間の判定はBoolean値を返す式として使います。

主なエラー: 未知の関数、引数個数の不一致、重複定義・重複/予約済み引数、関数内への関数・クラス定義、式として使った関数に戻り値がない、call depth超過。

<a id="return"></a>
## Return

構文: `Return expression` または値なしの `Return`。関数・メソッドを終了し、内側のIf / Forからも呼び出し元へ戻ります。

```rowly
Def Stop()
    Return
End Def
Stop()
```

主なエラー: トップレベルで実行したReturn、値なしで終わる関数・メソッドを式として利用。値なしReturnや末尾まで実行した呼び出しは、単独の文としてなら利用できます。

<a id="classes"></a>
## Class / Field / Method / New

構文: `Class name` ... `End Class`、内部の `Field name = expression` と `Def method(...)` ... `End Def`。メソッド定義にも `Def` を使います。生成は `New ClassName(arguments...)`、member操作はbinding名を起点に `object.field` / `object.field = expression` / `object.Method(...)` と書きます。終了表記 `EndClass` も受理します。

```rowly
Class Box
    Field value = "initial"
    Def Read()
        Return Self.value
    End Def
End Class
CONST box = New Box()
VAR alias = box
alias.value = "updated"
VAR result = box.Read()
```

Object代入は同じinstanceの別名参照になり、フィールド変更を共有します。CONSTはbindingを保護しますが、フィールドを凍結しません。フィールド既定値はNew時に評価します。

主なエラー: 未知のクラス・フィールド・メソッド、Object以外のmember操作、同一クラス内の重複定義、既定値なしのField、Class内の通常文。任意のmemberチェーンではなく、上記のbinding起点の形を使います。

<a id="self-init"></a>
## Self / Init

`Self` はメソッド・コンストラクタ内で対象instanceを指す再代入不能bindingです。`Def Init(...)` を定義するとNew時に自動呼び出しします。InitがなければNewの引数は0個です。

```rowly
Class Label
    Field text = ""
    Def Init(value)
        Self.text = value
    End Def
End Class
VAR label = New Label("完了")
```

主なエラー: コンストラクタ引数個数の不一致、呼び出しscope外のSelf参照、Selfの再代入、未知のフィールド。Newの結果はinstanceで、InitのReturn値は生成結果に使いません。

<a id="inheritance"></a>
## 単一継承 / Super

構文: `Class Child Extends Parent`。親→子の順にフィールドを初期化し、同名フィールド・メソッドは子を優先します。子にInitがなければ親のInitを継承します。子でInitを定義した場合、親Initを必要とするなら明示的に `Super.Init(...)` を呼びます。

```rowly
Class Base
    Field value = ""
    Def Init(value)
        Self.value = value
    End Def
    Def Read()
        Return Self.value
    End Def
End Class
Class Child Extends Base
    Def Init(value)
        Super.Init(value)
    End Def
    Def Read()
        Return Super.Read()
    End Def
End Class
VAR child = New Child("継承")
VAR result = child.Read()
```

`Super.Method(...)` の探索は、現在実行中のメソッドを**定義したクラスの親**から始まり、Selfは元instanceを維持します。

主なエラー: 親クラス不在、循環継承、メソッド外のSuper、親を持たないクラスのSuper、親階層に対象メソッドがない、引数個数不一致。

<a id="standard-functions"></a>
## 標準関数・名前空間

| 関数 | 引数 | 意味 |
| --- | --- | --- |
| `Text.Contains(value, needle)` | String×2 | 部分一致 |
| `Text.StartsWith(value, prefix)` | String×2 | 前方一致 |
| `Text.EndsWith(value, suffix)` | String×2 | 後方一致 |
| `Text.IsJapanese(value)` | String | 対象の日本語文字を1文字以上含む |
| `Number.IsInteger(value)` | 任意の値 | Integer、または整数として解釈可能なString |
| `Number.IsDecimal(value)` | 任意の値 | Integer / Decimal、または有限数として解釈可能なString |
| `Boolean.IsValid(value)` | 任意の値 | Boolean、またはtrue / falseのString |

すべてBooleanを返し、CSVを変更しません。Number / Booleanの不適合値はfalse、Textの非Stringは型エラーです。

```rowly
VAR hasName = Text.Contains(CellValue("A2"), "田")
VAR numeric = Number.IsInteger(CellValue("C2"))
VAR validBoolean = Boolean.IsValid("false")
```

主なエラー: 未知の標準関数、引数個数不一致、Textの非String。旧グローバル `Contains(...)` / `IsJapanese(...)` 等はbuiltinとして提供しません。名前解決・判定範囲は [DSL_STANDARD_LIBRARY.md](DSL_STANDARD_LIBRARY.md) を参照してください。

<a id="cells"></a>
## A1セル読み取り・範囲書き込み

構文: `CellValue("A2")`（1引数、Stringを返す）、`This.Worksheet.Editor.Cell(range).Value.Set = expression`。rangeには `A2`、`A2:B3`、未引用の `A2 To B3`、引用した `"A2:B3"` を使えます。範囲内の全セルに同じ値を書き、範囲全体で1つのUndo単位です。全セルの存在を検証してから変更します。

```rowly
VAR previous = CellValue("A2")
This.Worksheet.Editor.Cell(D2 To D3).Value.Set = "確認済み"
```

A1はヘッダーを含む実CSVの座標です。表示用行番号ではありません。主なエラー: 不正なA1/範囲、0の行番号、既存範囲外のセル、ragged row内の存在しないセル、Objectの書き込み。範囲選択で空行・空セルを自動生成しません。

<a id="dimensions"></a>
## 行列数取得・動的座標

| 関数 | 引数数 | 戻り値・動作 |
| --- | --- | --- |
| `RowCount()` | 0 | ヘッダーを含む実CSVレコード数（Integer） |
| `ColumnCount()` | 0 | 最も幅の広い行の列数（Integer） |
| `CellValueAt(row, column)` | 2 | セルのString |
| `SetCellValueAt(row, column, value)` | 3 | セル編集後のString |

row / columnには1以上のInteger値を渡します。ColumnCountまでの全セルが各行に存在するとは限りません。

```rowly
VAR rows = RowCount()
VAR columns = ColumnCount()
VAR name = CellValueAt(Integer("2"), Integer("1"))
SetCellValueAt(Integer("2"), Integer("4"), name)
```

主なエラー: 引数個数不一致、非Integer/0以下の座標、既存範囲外のセル。書き込み先の検証はprocess層で行います。

<a id="headers"></a>
## ヘッダー名ベース操作

| 関数 | 引数数 | 戻り値・動作 |
| --- | --- | --- |
| `ColumnIndex(header)` | 1 | 1-basedの列番号（Integer） |
| `CellValueByHeader(row, header)` | 2 | セルのString |
| `SetCellValueByHeader(row, header, value)` | 3 | セル編集後のString |

headerはStringで、先頭行から**完全一致**で検索します。rowはヘッダーを含む1-based Integerです。

```rowly
VAR column = ColumnIndex("状態")
VAR status = CellValueByHeader(Integer("2"), "状態")
SetCellValueByHeader(Integer("2"), "備考", status)
```

主なエラー: ヘッダー不在・重複、非Stringのheader、非Integer/0以下のrow、ragged row内の欠けたセル、引数個数不一致。読み取りはその時点の編集済みCSV内容を参照します。

<a id="column-checks"></a>
## 列の存在・タイトル・値チェック

selectorは固定の1-based列番号、または引用したヘッダー名です。動的な変数・式をselectorへ渡しません。

| 構文 | 意味 |
| --- | --- |
| `If This.Worksheet.Column(selector).Exists Then` | 列の存在条件 |
| `If This.Worksheet.Column(selector).Title = expression Then` | 列タイトルのString比較 |
| `This.Worksheet.Column(selector).Type = String` | 先頭行を除いた列値を非破壊チェック |
| `This.Worksheet.Column(selector).Check.Japanese` | 日本語文字を含む値・含まない値のレポート |

Typeに指定できるのは `String` / `Integer` / `Decimal` / `Boolean` です。このDSL文はデータの検証で、値の変換やViewer設定の永続化ではありません。結果は実行レポートに記録します。

```rowly
If This.Worksheet.Column("得点").Exists Then
    This.Worksheet.Column("得点").Type = Integer
End If
If This.Worksheet.Column(1).Title = "名前" Then
    This.Worksheet.Column(1).Check.Japanese
End If
```

主なエラー: selectorの形式不正・0、重複ヘッダー、存在しない列へのチェック、未知のType、タイトル比較に非String。値の型違反や日本語不一致自体はレポートの結果として扱います。

<a id="transactions"></a>
## transaction操作

構文: 単独の文として `BeginTransaction()` / `CommitTransaction()` / `RollbackTransaction()`（各0引数）。開始後のCSV編集をcommitすると1つのUndo単位になり、rollbackは逆順に復元して履歴を追加しません。

```rowly
BeginTransaction()
SetCellValueByHeader(Integer("2"), "状態", "進行中")
SetCellValueByHeader(Integer("2"), "備考", "更新")
CommitTransaction()
BeginTransaction()
This.Worksheet.Editor.Cell(D3).Value.Set = "一時変更"
RollbackTransaction()
```

主なエラー: 引数あり、ネスト開始、active transactionなしのcommit / rollback。active中はprocess側のUndo / Redo / Save / SaveAsを禁止します。

DSL実行エラー時にtransactionを自動rollbackする仕組みではありません。呼び出し側がtransaction状態を確認し、必要ならprocessの `rollback_transaction()` で復元してください。通常の編集も、それ以前の成功分を実行エラーだけで巻き戻しません。

<a id="validation-rules"></a>
## 入力規則の宣言

`SET` は列の `.Validation.AllowedValues` または `.Validation.Expression` に限って使えます。列セレクターは既存列の1-based番号または一意なヘッダー名です。AllowedValuesは文字列リテラルのリスト、Expressionは各比較に候補値 `Value` を含み、文字列リテラルだけを組み合わせるBoolean式です。比較・`Not`・`And`・`Or` を使えます。関数呼び出し、別の変数、セル参照などは使えません。

```rowly
SET This.Worksheet.Editor.Column("状態").Validation.AllowedValues = [
    "未着手",
    "進行中",
    "完了"
]

SET This.Worksheet.Editor.Column("状態").Validation.Expression =
    Value = "未着手" OR Value = "進行中" OR Value = "完了"
```

DSL実行時に列の存在・一意性を確認し、設定宣言をソース順の `ExecutionReport.validation_rules()` と `ValidationRuleSet` イベントで返します。`ValidationRule::matches(candidate)` はAllowedValuesとExpressionの両方を評価できます。この宣言処理自体はCSVを変更しません。ProjectSessionはsource IDと一意headerを付けたinit DSLとしてruleを保存・復元できます。保存・編集時の検証エンジンはprocess層が適用します。

<a id="limits"></a>
## CSVへ書き込める値と主要制限

セルへ書ける値はString / Integer / Decimal / Booleanです。Stringはそのまま、型付き値は文字列へ変換して保存対象とします。Objectは書けません。DSL内の型はCSVの永続型ではありません。

```rowly
This.Worksheet.Editor.Cell(D2).Value.Set = Boolean("true")
```

関数・メソッド・Initの同時呼び出し深さは最大64です。超過は `CallDepthExceeded`。Forの総反復数・DSL全体の実行時間には現在専用の制限がなく、Luauのsandbox制限をDSLにも適用したとは扱いません。

構文エラーは行番号付きのParseError、実行エラーは変数・型・呼び出し・CSV/processなどの原因を持つExecutionErrorとして報告します。正常終了時は変数・Objectフィールド・操作/チェックイベントをExecutionReportへ返します。CSV編集はprocess履歴に記録され、保存は呼び出し側の明示操作です。

<a id="unsupported"></a>
## 現在の構文と将来仕様の境界

本書の実行例は現在のparser/runtimeの形式です。入力規則以外の一般的な `SET target = value`、`CHECK ... IS ...`、`WITH`、`FOR EACH`、`WHERE`、`SAVE` は対応していません。標準関数・セル・列・transactionは本書の対応構文で記述してください。

実装との照合先: [parser](../src/rowly_dsl/parser.rs)、[runtime](../src/rowly_dsl/runtime.rs)、[AST](../src/rowly_dsl/ast.rs)。`tests/dsl_reference_examples.rs` は本書の独立した最小例を共通CSV上で解析・実行し、実装との整合を確認します。
