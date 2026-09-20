# アーキテクチャ

## 目的

Rowly は CSV-first のエディタです。表示、スクリプト、外部連携のコードが第二の正本を作らない構造を維持します。

## 依存方向

```text
UI / Rowly DSL / Luau / Python adapters
                |
                v
        process/application
                |
                v
              data
```

`data` モジュールは正本となる表データと CSV／文字コード処理を担当します。
`process` モジュールは open、edit、save、A1 参照、列の意味解釈など、ユーザーから見える操作を統括します。
`rowly_dsl` と `luau` は `process` 境界を通じて操作し、CSV コーデックへ直接アクセスしません。
将来の UI や Python アダプタも同じ依存方向に従います。

この方針は UPD Commander Base Design の責務分離と依存方向の考え方を参考にしますが、Java 的な Commander/Messenger 構造を Rust へ機械的に移植しません。Rust のモジュール、可視性、狭い API を使って同じ目的をより少ない儀式で実現します。

## Data 層

責務:
- CSV の行／セルを文字列として保持する
- 対応文字コードを内部 UTF-8 へ変換する
- CSV レコードを解析する
- 正本 CSV を書き出す

責務外:
- UI 状態
- 表示上のグループ化
- スクリプト構文やランタイムオブジェクト
- Excel 固有概念
- 推論された表示型

先頭行は正本モデル上では特別扱いしません。ヘッダー解釈は上位層の責務です。

## Process / application 層

責務:
- open/edit/save の調停
- UI・スクリプトアダプタへ安定した操作を公開する
- dirty 状態やパスなどセッション状態を管理する
- A1／範囲編集、行列の構造編集を提供する
- 複数編集を1つの履歴操作として commit / rollback する transaction API を提供する
- ヘッダー検索と非破壊な列意味チェックを提供する
- data 層のエラーをアプリ向けエラーへ変換する

責務外:
- CSV バイト解析／文字コード詳細
- 描画
- DSL 構文解析／評価やオブジェクト寿命
- Luau 言語ランタイムの内部仕様
- Excel 実装詳細

処理が大きくなった場合は、小さな processing module へ分割します。オーケストレータは調停に集中し、実装詳細を抱え込まないようにします。

### Process transaction

`CsvDocument` の transaction はメモリ上の編集履歴をまとめるための process API です。ネストは許可しません。transaction 中の編集は即座に正本テーブルへ反映されますが、commit 時に1つの履歴コマンドへ集約され、1回の undo/redo で全体を戻す／進めることができます。rollback は transaction 内の編集を逆順に戻し、履歴には残しません。

transaction 中は履歴位置や保存基準を壊さないため `undo` / `redo` / `save` / `save_as` を禁止します。未commitの実変更がある間は dirty とみなします。これはDB transactionではなく、CSV-first編集セッション上の原子的な操作単位です。

## Rowly DSL アダプタ

`rowly_dsl` は `process` 上に載るユーザー向けマクロ言語です。AST、parser、runtime を分離し、言語拡張が data 層へ漏れないようにします。

現在の責務:
- `If ... Then` / `Else` / `End If` の制御構文
- `Not` / `And` / `Or` と比較演算子
- `Let`、トップレベル `Def ...` / `End Def`、呼び出し、引数、`Return`
- `Class ...` / `End Class`、`Class Child Extends Parent` の単一継承、`Field`、method、`New ClassName(args...)`、member 読み書き
- `Def Init(...)` を生成時に自動実行する引数付きコンストラクタ
- global scope と関数／method ごとの local scope
- method 実行時だけの `Self`
- DSL runtime 内だけの class instance / object identity
- `Integer(...)` / `Decimal(...)` / `Boolean(...)` / `String(...)` の明示変換
- Integer / Decimal の数値比較、文字列の辞書順比較
- `+` / `-` / `*` / `/`、単項 `-`、括弧を持つ型付き算術式と演算子優先順位
- `Contains` / `StartsWith` / `EndsWith` / `IsJapanese` / `IsInteger` / `IsDecimal` / `IsBoolean` の値判定
- Boolean 値を返す式を直接 `If` 条件として評価する
- `CellValue("A1")` による process 境界経由の現在セル値読み取り
- `For ... To ... [Step ...]` / `Next` の整数ループとループ専用スコープ
- `RowCount()` / `ColumnCount()` / `CellValueAt(...)` / `SetCellValueAt(...)` の1-based動的セル操作
- `ColumnIndex(...)` / `CellValueByHeader(...)` / `SetCellValueByHeader(...)` による一意ヘッダー経由の動的セル操作
- `BeginTransaction()` / `CommitTransaction()` / `RollbackTransaction()` による process transaction の明示操作
- `Return` のネスト制御フロー伝播
- 暴走再帰を防ぐ call depth 上限
- `This.Worksheet.Column(...)` / `This.Worksheet.Editor.Cell(...)` を process API へ対応付ける
- 実行レポートの提供
- 上から下へ実行するマクロ semantics の維持

DSL の runtime object は CSV 正本モデルの一部ではありません。CSV への作用は必ず process API を通します。型付きスカラーを CSV セルへ書く場合も process 境界で文字列へ変換します。

Rowly DSL の transaction 制御も独自履歴を持たず、`CsvDocument` の transaction API を直接仲介します。commit 後は transaction 全体が1つの undo/redo 単位になり、rollback は process 層の規則に従って履歴を残さず復元します。

`Init` コンストラクタと単一継承は実装済みです。継承時は親から子の順にフィールドを初期化し、同名フィールド／メソッド／`Init` は子側を優先します。存在しない親と循環継承はエラーにします。`Super.Method(...)` / `Super.Init(...)` は現在実行中のメソッドを定義したクラスの親から探索し、`Self` のオブジェクト同一性を維持します。

DSL の列番号はユーザー向けに 1-based、process/data の内部 index は 0-based です。

## Luau アダプタ

`luau` は自由度の高いユーザースクリプト用の組み込みアダプタです。Luau VM は `mlua` の Luau backend を使用します。

Luau にはグローバル `Rowly` テーブルを公開し、現時点では次の process 操作だけを提供します。

- `Rowly.cell(reference)`
- `Rowly.set_cell(reference, value)`
- `Rowly.set_range(range, value)`
- `Rowly.row_count()`
- `Rowly.column_count()`
- `Rowly.undo()` / `Rowly.redo()`
- `Rowly.begin_transaction()` / `Rowly.commit_transaction()` / `Rowly.rollback_transaction()`

Luau から `data` や CSV codec へ直接アクセスさせません。不正な A1 参照など process 層のエラーは Luau runtime error として伝播します。

Luau 側から CSV へ書き込める値は現時点では文字列だけです。table、function、userdata などを暗黙に CSV 文字列へ変換してはなりません。

Luau の transaction API も `CsvDocument` を直接仲介し、adapter 独自の履歴モデルを持ちません。スクリプト開始時に transaction が無かった場合に限り、スクリプトが開始した未commit transaction を runtime error や実行制限停止時の終了処理で rollback します。外部で開始済みの transaction や commit 済み編集は自動 rollback しません。

Luau ユーザースクリプトは `Lua::set_interrupt` による実行時間／interrupt回数制限と、Luau VM allocator のメモリ上限を必ず設定して実行します。既定値は5秒、100万interrupt、64 MiBです。用途別の上限は `LuauLimits` で差し替えます。interrupt回数はLuau命令数そのものではありません。詳細仕様は [`LUAU.md`](LUAU.md) を正本とします。

## Rowly sidecar メタデータ

列型宣言など Rowly 固有の補助情報は CSV 本体へ埋め込まず、`<csv>.rowly.json` sidecar に保存します。CSV 単体で表データを完全に復元できることを不変条件とし、sidecar は正本ではありません。

現時点では `version: 1` と、ヘッダー名をキーにした列型宣言だけを保持します。宣言設定時は既存の一意ヘッダー検索を使うため、重複ヘッダーは対象にできません。列順が変わってもヘッダー名で再解決します。

sidecar が存在しない場合は空メタデータとして扱います。不正な JSON や未知の型・version がある場合も CSV の open 自体は成功させ、`metadata_error()` から補助情報の読み込み失敗を確認できるようにします。

## 文字コード方針

- 内部表現は UTF-8 の Rust `String`。
- UTF-8 入力を受け付ける。UTF-8 BOM も対応する。
- Shift_JIS は読み込み時に検出し UTF-8 へ変換する。
- 対応外と判断した文字コードは黙って誤変換せずエラーにする。
- 保存時は UTF-8 とする。

Shift_JIS → UTF-8 変換をユーザーへどう通知するかは GUI 実装時に決めます。

## CSV fidelity

Rowly は一般的なCSVの空値、標準引用符、引用符内カンマ、二重引用符エスケープ、引用符内改行、LF/CRLFを読み込み対象とします。独自のCSVエスケープ方式は導入しません。保存時はUTF-8かつLFへ正規化します。

Rowly は、行と文字列セルからなる表の意味を保持します。保存時には quoting や改行コードなどのバイト表現が正規化される場合がありますが、解析後のレコードと値を維持します。

byte-for-byte の完全 round trip は要件ではありません。空行や dialect の忠実性が将来重要になった場合は、UI metadata へ隠さず data 層で明示的に扱います。

## Excel Python アダプタ

`excel_python` は Python / openpyxl を使う `.xlsx` 交換アダプタです。Python 側は CSV codec や Rowly の data 層を直接扱いません。export は `CsvDocument` が process 境界から公開した行文字列を JSON で bridge へ渡し、import は bridge が返した行文字列を `CsvDocument::create` で UTF-8 CSV 正本として作成します。

Excel は交換経路であり、開いた `.xlsx` を第二の正本として保持しません。export 時の CSV セルは Excel でも文字列として書き込みます。import 時の値は canonical CSV 文字列へ明示変換し、Excel の型・書式・数式計算結果を Rowly の正本モデルへ持ち込みません。

現時点では単一シートのみを扱い、シート名指定がなければ active sheet を読み込みます。merged cell、複数シート統合、書式保持は Rowly のデータモデルへ暗黙導入しません。詳細は [`EXCEL.md`](EXCEL.md) を正本とします。

## 今後の境界

- `ui`: 具体的な viewer/editor と描画。process のみに依存する。

空の抽象化層は先に作らず、必要になった時点で追加します。

## 文書の言語

この日本語版をアーキテクチャ仕様の正本とします。英語版を用意する場合も、差異があるときは日本語版を優先します。
