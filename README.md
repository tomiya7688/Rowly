# Rowly

Rowly は、次の原則を中心に設計する表ビューア／エディタです。

> **CSV を正本とする。**

## 基本方針

### CSV を正本とする

Rowly は CSV を独自ドキュメント形式へ取り込み、その独自形式を正本にはしません。
CSV 自体を常に正本データとして扱います。

表示や利便性のためだけの機能が、元の CSV の意味や構造を変更してはなりません。
Rowly 固有のメタデータを導入する場合も補助情報に限定し、実データの復元に必須としてはなりません。

### 結合セルを持たない

Rowly のデータモデルには結合セルを持ちません。
表は CSV と同じく、行と列からなる平坦な構造を維持します。

### 重複値のグループ化は表示だけで行う

同じ列で同じ値が連続する場合、表示上は一つのグループとして見せても構いません。
ただし CSV 自体は一切変更せず、各行が元の値を保持します。

> **見た目を良くするためだけにデータ構造を変えない。**

## 現在の実装

Rust コアでは次を実装済みです。

- UTF-8 CSV の読み込み／保存
- Shift_JIS の検出と読み込み時の UTF-8 変換
- 保存時の UTF-8 出力
- 文字列ベースの行／セルモデル
- `A1`、`AA10` などの A1 セル参照
- `A1:A8`、`A1:B4` などの矩形範囲
- 範囲編集の原子性
- 行の挿入／削除
- 不揃いな行を保持した列の挿入／削除
- セル編集・構造編集の undo/redo
- 複数編集を1回の undo/redo にまとめる process transaction API
- 保存状態を考慮した dirty 管理
- 先頭行を明示的にヘッダーとして扱う検索と重複検出
- `String` / `Integer` / `Decimal` / `Boolean` の非破壊列チェック
- 日本語文字チェックと A1 参照による結果報告
- process API 上で動作する BASIC 風 Rowly DSL
- Rowly DSL の変数、関数、引数、戻り値、ローカルスコープ
- `If` / `Else`、`Not` / `And` / `Or`、`!=` / `<` / `<=` / `>` / `>=`
- Rowly DSL のクラス、単一継承、インスタンス、フィールド、メソッド、`Self`、`New ClassName(args...)`、`Init` コンストラクタ
- `Integer(...)` / `Decimal(...)` / `Boolean(...)` / `String(...)` による明示変換
- Integer / Decimal の数値比較と、文字列の辞書順比較
- Rowly DSL の算術式 `+` / `-` / `*` / `/`、単項 `-`、括弧、演算子優先順位
- `Contains` / `StartsWith` / `EndsWith` / `IsJapanese` / `IsInteger` / `IsDecimal` / `IsBoolean` 組み込み判定
- `CellValue("A2")` による CSV セル値の式読み取り
- `For ... To ... [Step ...]` / `Next` による整数ループ
- `RowCount()` / `ColumnCount()` / `CellValueAt(row, column)` / `SetCellValueAt(row, column, value)` による動的セル操作
- `ColumnIndex(header)` / `CellValueByHeader(row, header)` / `SetCellValueByHeader(row, header, value)` によるヘッダー名ベース操作
- Luau ユーザースクリプト実行アダプタ
- process レベルの open/edit/save API
- ヘッドレス CLI のスモークエントリポイント

列型チェックは意味解釈／検証のみであり、CSV の正本文字列を書き換えません。

`CsvDocument::begin_transaction()` / `commit_transaction()` / `rollback_transaction()` で複数編集を1つの履歴操作にまとめられます。transaction 中は `undo` / `redo` / `save` / `save_as` を禁止し、rollback は履歴を増やさず変更を元へ戻します。

## スクリプト

### Rowly DSL

Rowly DSL はアプリ操作を直接表現する専用言語です。
`If ... Then` / `Else` / `End If`、`Let`、`Def ...` / `End Def`、`Return`、関数、クラス、フィールド、メソッド、列チェック、範囲値設定を扱います。

```text
Class Formatter
    Field replacement = "佐藤"

    Def Value()
        Return Self.replacement
    End Def
End Class

Let formatter = New Formatter()
This.Worksheet.Editor.Cell(A2 To A8).Value.Set = formatter.Value()
```

明示変換により、既存の文字列リテラルの挙動を変えずに型付きランタイム値を扱えます。

```text
Let small = Integer("2")
Let large = Integer("10")
Let threshold = Decimal("9.5")
Let enabled = Boolean("true")

If large > small And threshold < large Then
    This.Worksheet.Editor.Cell(A2).Value.Set = large
End If
```

文字列は辞書順、Integer / Decimal は数値として比較します。Integer と Decimal の相互比較も可能です。Boolean は等値／不等値比較のみです。CSV セルへ型付きスカラーを書き込む場合は process 境界で文字列へ変換し、CSV の正本性を維持します。

算術式は `-x`、`*` / `/`、`+` / `-`、比較、`And`、`Or` の順に優先されます。Integer 同士の `+` / `-` / `*` は Integer、Integer / Decimal 混在は Decimal、`/` は常に Decimal です。0 除算、整数 overflow、非数値への算術は暗黙変換せずエラーにします。

値単位の判定には `Contains` / `StartsWith` / `EndsWith` / `IsJapanese` / `IsInteger` / `IsDecimal` / `IsBoolean` を使用できます。Boolean を返す式は `If IsJapanese(value) Then` のように比較演算子なしで条件として直接使用できます。`IsJapanese` は文字列中に日本語文字を1文字以上含むかを判定します。

CSV の既存セル値は `CellValue("A2")` で文字列として読み取れます。読み取りも `process::CsvDocument` を経由し、同じスクリプト内で先に行った編集結果を直後の式から参照できます。不正な A1 参照や存在しないセルは明示エラーです。

全行処理には `For row = Integer("2") To RowCount()` / `Next row` を使用できます。`Step` は省略時1で、負数による降順ループにも対応します。`CellValueAt` / `SetCellValueAt` の行・列番号は1-basedです。ループ変数とループ内 `Let` はループ専用スコープに限定され、終了後は外側へ漏れません。

列番号を固定したくない場合は `ColumnIndex("名前")`、`CellValueByHeader(row, "名前")`、`SetCellValueByHeader(row, "状態", value)` を使用できます。ヘッダー検索は先頭行を完全一致で検索し、見つからない場合や重複して一意に決められない場合はエラーにします。`ColumnIndex` の返り値はDSL上の1-based列番号です。

クラス内に `Def Init(...)` を定義すると、`New ClassName(args...)` の生成時にフィールド初期値を作成した後、`Self` をその新規インスタンスへ束縛して自動実行します。`Init` が無いクラスは従来どおり引数なしで生成でき、余分な引数や不足した引数は明示エラーになります。

単一継承は `Class Child Extends Parent` で定義します。フィールド初期値は親から子の順に適用し、子の同名フィールドが親を上書きします。メソッドと `Init` も子側を優先して探索するため、子で定義しなければ親の実装を継承します。存在しない親クラスと循環継承は明示エラーになります。

上書きしたメソッドや `Init` から親実装を呼ぶ場合は `Super.Method(...)` / `Super.Init(...)` を使用します。`Super` は現在実行中のメソッドを定義したクラスの親から探索し、`Self` は元の子インスタンスを維持します。

オブジェクト変数は DSL ランタイム内部だけの参照です。クラスインスタンスが CSV に代わる正本になることはありません。CSV への作用は必ず process API を通します。

### Luau

Luau は自由度の高いユーザースクリプト用です。現時点ではグローバル `Rowly` テーブルを通じて、セル読み取り、単一セル／範囲編集、行列数取得、undo/redo を利用できます。

```luau
assert(Rowly.cell("A2") == "Alice")
Rowly.set_cell("B2", "42")
Rowly.set_range("A3:B3", "updated")
```

Luau からも CSV コーデックや `data` 層へ直接アクセスせず、すべて `process::CsvDocument` を経由します。
Luau 連携の正本仕様は [`docs/LUAU.md`](docs/LUAU.md) を参照してください。

## 未実装

- GUI
- Python/Excel ブリッジ
- 永続的な列メタデータ／型宣言
- より広い型推論
- 表示上のグループ化
- Luau の実行時間・命令数・メモリ量の制限

アーキテクチャと依存方向は [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) を参照してください。

## 文書の言語

この日本語 README を正本とします。英語版を用意する場合も、内容に差異があるときは日本語版を優先します。
