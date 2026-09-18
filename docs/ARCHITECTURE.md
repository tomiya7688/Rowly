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
- ヘッダー検索と非破壊な列意味チェックを提供する
- data 層のエラーをアプリ向けエラーへ変換する

責務外:
- CSV バイト解析／文字コード詳細
- 描画
- DSL 構文解析／評価やオブジェクト寿命
- Luau 言語ランタイムの内部仕様
- Excel 実装詳細

処理が大きくなった場合は、小さな processing module へ分割します。オーケストレータは調停に集中し、実装詳細を抱え込まないようにします。

## Rowly DSL アダプタ

`rowly_dsl` は `process` 上に載るユーザー向けマクロ言語です。AST、parser、runtime を分離し、言語拡張が data 層へ漏れないようにします。

現在の責務:
- `If ... Then` / `Else` / `End If` の制御構文
- `Not` / `And` / `Or` と比較演算子
- `Let`、トップレベル `Def ...` / `End Def`、呼び出し、引数、`Return`
- `Class ...` / `End Class`、`Field`、method、`New ClassName()`、member 読み書き
- global scope と関数／method ごとの local scope
- method 実行時だけの `Self`
- DSL runtime 内だけの class instance / object identity
- `Integer(...)` / `Decimal(...)` / `Boolean(...)` / `String(...)` の明示変換
- Integer / Decimal の数値比較、文字列の辞書順比較
- `+` / `-` / `*` / `/`、単項 `-`、括弧を持つ型付き算術式と演算子優先順位
- `Contains` / `StartsWith` / `EndsWith` / `IsJapanese` / `IsInteger` / `IsDecimal` / `IsBoolean` の値判定
- Boolean 値を返す式を直接 `If` 条件として評価する
- `Return` のネスト制御フロー伝播
- 暴走再帰を防ぐ call depth 上限
- `This.Worksheet.Column(...)` / `This.Worksheet.Editor.Cell(...)` を process API へ対応付ける
- 実行レポートの提供
- 上から下へ実行するマクロ semantics の維持

DSL の runtime object は CSV 正本モデルの一部ではありません。CSV への作用は必ず process API を通します。型付きスカラーを CSV セルへ書く場合も process 境界で文字列へ変換します。

引数付きコンストラクタと継承は今後の拡張です。

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

Luau から `data` や CSV codec へ直接アクセスさせません。不正な A1 参照など process 層のエラーは Luau runtime error として伝播します。

Luau 側から CSV へ書き込める値は現時点では文字列だけです。table、function、userdata などを暗黙に CSV 文字列へ変換してはなりません。

Luau の実行時間・命令数・メモリ量の制限は未実装です。詳細仕様は [`LUAU.md`](LUAU.md) を正本とします。

## 文字コード方針

- 内部表現は UTF-8 の Rust `String`。
- UTF-8 入力を受け付ける。UTF-8 BOM も対応する。
- Shift_JIS は読み込み時に検出し UTF-8 へ変換する。
- 対応外と判断した文字コードは黙って誤変換せずエラーにする。
- 保存時は UTF-8 とする。

Shift_JIS → UTF-8 変換をユーザーへどう通知するかは GUI 実装時に決めます。

## CSV fidelity

Rowly は、行と文字列セルからなる表の意味を保持します。保存時には quoting や改行コードなどのバイト表現が正規化される場合がありますが、解析後のレコードと値を維持します。

byte-for-byte の完全 round trip は要件ではありません。空行や dialect の忠実性が将来重要になった場合は、UI metadata へ隠さず data 層で明示的に扱います。

## 今後の境界

- `ui`: 具体的な viewer/editor と描画。process のみに依存する。
- `excel_python`: Python-backed Excel import/export。Excel は交換経路であり、開いた CSV に代わる正本にはしない。

空の抽象化層は先に作らず、必要になった時点で追加します。

## 文書の言語

この日本語版をアーキテクチャ仕様の正本とします。英語版を用意する場合も、差異があるときは日本語版を優先します。
