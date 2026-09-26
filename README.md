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
- 一般的なCSVの空値、標準引用符、引用符内カンマ／改行、LF／CRLF読み込み
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
- `<csv>.rowly.json` sidecar によるヘッダー単位の永続列型宣言
- versioned `.rwprj` JSON manifest によるCSV source／script／history参照の保存と読込
- `.rowlyx` ZIP互換プロジェクトのpack／open／extract
- `String` / `Integer` / `Decimal` / `Boolean` の非破壊列チェック
- 日本語文字チェックと A1 参照による結果報告
- process API 上で動作する BASIC 風 Rowly DSL
- Rowly DSL の `VAR` / `CONST` 宣言、再代入、関数、引数、戻り値、ローカルスコープ
- `If` / `Else`、`Not` / `And` / `Or`、`!=` / `<` / `<=` / `>` / `>=`
- Rowly DSL のクラス、単一継承、インスタンス、フィールド、メソッド、`Self`、`New ClassName(args...)`、`Init` コンストラクタ
- `Integer(...)` / `Decimal(...)` / `Boolean(...)` / `String(...)` による明示変換
- Integer / Decimal の数値比較と、文字列の辞書順比較
- Rowly DSL の算術式 `+` / `-` / `*` / `/`、単項 `-`、括弧、演算子優先順位
- `Text.Contains` / `Text.StartsWith` / `Text.EndsWith` / `Text.IsJapanese` / `Number.IsInteger` / `Number.IsDecimal` / `Boolean.IsValid` の名前空間付き標準判定
- `CellValue("A2")` による CSV セル値の式読み取り
- `For ... To ... [Step ...]` / `Next` による整数ループ
- `RowCount()` / `ColumnCount()` / `CellValueAt(row, column)` / `SetCellValueAt(row, column, value)` による動的セル操作
- `ColumnIndex(header)` / `CellValueByHeader(row, header)` / `SetCellValueByHeader(row, header, value)` によるヘッダー名ベース操作
- `BeginTransaction()` / `CommitTransaction()` / `RollbackTransaction()` による process transaction 操作
- Luau ユーザースクリプト実行アダプタ、process transaction 操作、実行時間・interrupt回数・メモリ量の強制制限
- Python / openpyxl バックエンドによる単一シート Excel (.xlsx) import/export
- Python runtime / openpyxl を自己完結 helper として同梱する Excel 配布構成
- CLI からの Excel import/export
- process レベルの open/edit/save API
- ヘッドレス CLI のスモークエントリポイント

列型チェックは意味解釈／検証のみであり、CSV の正本文字列を書き換えません。

`.rwprj` は project name と宣言済みの source、script、history への参照だけを保持します。CSV内容は埋め込みません。相対パスは project file のあるディレクトリを基準に解決します。`LogicalProject::load` は宣言済みのCSV fileとdirectoryだけを読み、directory sourceは`recursive`設定に従ってCSVを列挙します。列名と順序が一致するsourceは同じlogical tableにまとめ、各行にstable source id・元file path・file内record indexを保持します。表示順は元sourceの識別情報から独立し、logical combineでCSVファイルを変更・結合しません。

`.rowlyx` は通常の `.rwprj` projectをZIP互換コンテナへ包装します。プロジェクト内の相対参照先だけを収録し、絶対パスの参照は外部参照として維持します。open時にarchive entryとproject定義を検証し、extract時はpath traversalとsymlinkを拒否します。

論理テーブルでは、参加するstable source idの中からdefault write targetを明示できます。未指定のsourceや別logical tableのsourceはtargetとして受け付けません。

CSVの読み書きは独自方言を追加せず、標準的な引用符規則を使用します。空値、引用符内のカンマ、`""` による引用符エスケープ、引用符内改行を値として保持し、LF/CRLFの双方を読み込めます。保存時はUTF-8/LFへ正規化します。

`CsvDocument::begin_transaction()` / `commit_transaction()` / `rollback_transaction()` で複数編集を1つの履歴操作にまとめられます。transaction 中は `undo` / `redo` / `save` / `save_as` を禁止し、rollback は履歴を増やさず変更を元へ戻します。

## スクリプト

### Rowly DSL

Rowly DSL はアプリ操作を直接表現する専用言語です。
`If ... Then` / `Else` / `End If`、`VAR` / `CONST`、`Def ...` / `End Def`、`Return`、関数、クラス、フィールド、メソッド、列チェック、範囲値設定を扱います。

```text
Class Formatter
    Field replacement = "佐藤"

    Def Value()
        Return Self.replacement
    End Def
End Class

VAR formatter = New Formatter()
This.Worksheet.Editor.Cell(A2 To A8).Value.Set = formatter.Value()
```

変数は `VAR`、再代入しない名前は `CONST` で初期値付き宣言を行い、再代入は宣言キーワードなしで書きます。

```text
CONST increment = Integer("2")
VAR count = Integer("1")
count = count + increment
```

`CONST` への再代入、未宣言名への代入、同じスコープでの再宣言は明示エラーです。禁止された代入では右辺を評価しません。`CONST` は名前の束縛を保護するもので、オブジェクトのフィールドを凍結する指定ではありません。旧 `LET` / `DIM` 宣言は構文エラーになります。移行方法とスコープ規則は [`docs/DSL_BINDINGS.md`](docs/DSL_BINDINGS.md) を日本語正本とします。

明示変換により、既存の文字列リテラルの挙動を変えずに型付きランタイム値を扱えます。

```text
VAR small = Integer("2")
VAR large = Integer("10")
VAR threshold = Decimal("9.5")
VAR enabled = Boolean("true")

If large > small And threshold < large Then
    This.Worksheet.Editor.Cell(A2).Value.Set = large
End If
```

文字列は辞書順、Integer / Decimal は数値として比較します。Integer と Decimal の相互比較も可能です。Boolean は等値／不等値比較のみです。CSV セルへ型付きスカラーを書き込む場合は process 境界で文字列へ変換し、CSV の正本性を維持します。

算術式は `-x`、`*` / `/`、`+` / `-`、比較、`And`、`Or` の順に優先されます。Integer 同士の `+` / `-` / `*` は Integer、Integer / Decimal 混在は Decimal、`/` は常に Decimal です。0 除算、整数 overflow、非数値への算術は暗黙変換せずエラーにします。

値単位の判定は標準名前空間から呼び出します。`Text.Contains` / `Text.StartsWith` / `Text.EndsWith` / `Text.IsJapanese`、`Number.IsInteger` / `Number.IsDecimal`、`Boolean.IsValid` を使用できます。Boolean を返す式は `If Text.IsJapanese(value) Then` のように比較演算子なしで条件として直接使用できます。旧グローバル `Contains(...)` / `IsJapanese(...)` 等は 1.0 構文ではありません。詳細は [`docs/DSL_STANDARD_LIBRARY.md`](docs/DSL_STANDARD_LIBRARY.md) を参照してください。

CSV の既存セル値は `CellValue("A2")` で文字列として読み取れます。読み取りも `process::CsvDocument` を経由し、同じスクリプト内で先に行った編集結果を直後の式から参照できます。不正な A1 参照や存在しないセルは明示エラーです。

全行処理には `For row = Integer("2") To RowCount()` / `Next row` を使用できます。`Step` は省略時1で、負数による降順ループにも対応します。`CellValueAt` / `SetCellValueAt` の行・列番号は1-basedです。ループ変数とループ内の `VAR` / `CONST` は反復ごとの専用スコープに限定され、次の反復やループの外側へ漏れません。反復をまたぐ集計変数はループの前に `VAR` で宣言し、ループ内で再代入します。

列番号を固定したくない場合は `ColumnIndex("名前")`、`CellValueByHeader(row, "名前")`、`SetCellValueByHeader(row, "状態", value)` を使用できます。ヘッダー検索は先頭行を完全一致で検索し、見つからない場合や重複して一意に決められない場合はエラーにします。`ColumnIndex` の返り値はDSL上の1-based列番号です。

複数の DSL 編集を1つの undo/redo 単位にまとめる場合は `BeginTransaction()` で開始し、`CommitTransaction()` で確定します。`RollbackTransaction()` は transaction 内の編集を逆順に元へ戻し、履歴を追加しません。transaction のネストや active transaction がない状態での commit / rollback は process 層の明示エラーになります。

クラス内に `Def Init(...)` を定義すると、`New ClassName(args...)` の生成時にフィールド初期値を作成した後、`Self` をその新規インスタンスへ束縛して自動実行します。`Init` が無いクラスは従来どおり引数なしで生成でき、余分な引数や不足した引数は明示エラーになります。

単一継承は `Class Child Extends Parent` で定義します。フィールド初期値は親から子の順に適用し、子の同名フィールドが親を上書きします。メソッドと `Init` も子側を優先して探索するため、子で定義しなければ親の実装を継承します。存在しない親クラスと循環継承は明示エラーになります。

上書きしたメソッドや `Init` から親実装を呼ぶ場合は `Super.Method(...)` / `Super.Init(...)` を使用します。`Super` は現在実行中のメソッドを定義したクラスの親から探索し、`Self` は元の子インスタンスを維持します。

オブジェクト変数は DSL ランタイム内部だけの参照です。クラスインスタンスが CSV に代わる正本になることはありません。CSV への作用は必ず process API を通します。

### Luau

Luau は自由度の高いユーザースクリプト用です。現時点ではグローバル `Rowly` テーブルを通じて、セル読み取り、単一セル／範囲編集、行列数取得、transaction 操作を利用できます。履歴HEADを直接動かす `undo` / `redo` は Luau へ公開しません。

```luau
assert(Rowly.cell("A2") == "Alice")
Rowly.set_cell("B2", "42")
Rowly.set_range("A3:B3", "updated")
```

Luau からも CSV コーデックや `data` 層へ直接アクセスせず、すべて `process::CsvDocument` を経由します。
ユーザースクリプトは既定で実行時間5秒、VM interrupt 100万回、Luau VMメモリ64 MiBの上限付きで実行し、無限ループや過剰なメモリ確保を停止します。
`Rowly.begin_transaction()` / `commit_transaction()` / `rollback_transaction()` で process transaction を明示操作できます。スクリプト自身が開始した transaction を未commitのまま実行エラーや強制停止で終了した場合は、active transaction を取り残さないよう終了処理で rollback します。
Luau 連携の正本仕様は [`docs/LUAU.md`](docs/LUAU.md) を参照してください。

## Excel 連携

Excel は交換経路であり、Rowly の正本にはしません。正式配布では Python runtime と openpyxl を自己完結 Excel backend として同梱するため、通常ユーザーへ Python / pip の導入を要求しません。`excel_python` アダプタは `CsvDocument` の行データを Python / openpyxl へ渡して `.xlsx` を生成し、import 時はワークシート値を Rust 側へ戻して UTF-8 CSV 正本を新規作成します。CSV の文字列を Excel 側で勝手に数値化せず、export は文字列セルとして出力します。

CLI からは次の形で利用できます。

```text
rowly excel import input.xlsx output.csv [sheet-name]
rowly excel export input.csv output.xlsx [sheet-name]
```

import はシート名省略時に active sheet、export は省略時に `Sheet1` を使用します。従来の `rowly <csv-path>` による CSV 情報表示も維持します。

詳細仕様は [`docs/EXCEL.md`](docs/EXCEL.md) を参照してください。

## 列メタデータ

列型宣言は CSV 本体へ埋め込まず、同じ場所の `<csv>.rowly.json` sidecar に補助情報として保存します。キーは一意なヘッダー名で、`String` / `Integer` / `Decimal` / `Boolean` を宣言できます。sidecar が無くても CSV は完全に開け、sidecar が壊れていても CSV の読み込み自体は失敗しません。

## 未実装

- GUI
- より広い型推論
- 表示上のグループ化

アーキテクチャと依存方向は [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) を参照してください。

## ライセンス

Rowly のソースコードは [Apache License 2.0](LICENSE) の下で公開します。

商用利用、改造、再配布、プロプライエタリな派生物での利用を含め、Apache-2.0 が許可する範囲で利用できます。特許ライセンス、再配布時の表示、商標、無保証・責任制限などの条件は `LICENSE` 全文に従います。

Rowly に同梱する第三者コンポーネントにはそれぞれのライセンスが適用されます。設計原則など、コード以外の文書に別ライセンスを明示した場合は、その文書に表示された条件を優先します。

## 文書の言語

この日本語 README を正本とします。英語版を用意する場合も、内容に差異があるときは日本語版を優先します。
