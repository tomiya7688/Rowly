# Luau 連携仕様

## 目的

Luau は、Rowly 上で自由度の高いユーザースクリプトを実行するための組み込み言語として使用する。
Rowly DSL がアプリ操作を直接表現するための専用言語であるのに対し、Luau は条件分岐、関数、テーブルなどを使った汎用的な自動化を担う。

## 境界

Luau から CSV のデータ層へ直接アクセスしてはならない。
すべての CSV 操作は `process` 層の `CsvDocument` を経由する。

```text
Luau script
    |
    v
luau adapter
    |
    v
process::CsvDocument
    |
    v
data
```

この制約により、Luau 経由の編集も GUI や Rowly DSL と同じ編集履歴、検証、dirty 状態管理を利用する。

## 公開 API

Luau スクリプトにはグローバルテーブル `Rowly` を公開する。
現時点の API は次のとおり。

- `Rowly.cell(reference)` — A1 形式のセル値を取得する。セルが存在しない場合は `nil`。
- `Rowly.set_cell(reference, value)` — A1 形式で単一セルへ文字列を書き込む。
- `Rowly.set_range(range, value)` — A1 形式の矩形範囲へ同一文字列を書き込む。
- `Rowly.row_count()` — 現在の行数を返す。
- `Rowly.column_count()` — 現在の最大列数を返す。
- `Rowly.undo()` — 1 操作戻す。戻せた場合は `true`。
- `Rowly.redo()` — 1 操作進める。進められた場合は `true`。

例:

```luau
assert(Rowly.cell("A2") == "Alice")
Rowly.set_cell("B2", "42")
Rowly.set_range("A3:B3", "updated")
```

## データ規則

CSV の正本データは文字列である。
Luau からセルへ書き込む値も現時点では文字列だけを受け付ける。
Lua/Luau のテーブル、関数、userdata などを暗黙に CSV 文字列へ変換して保存してはならない。

A1 参照の解釈、範囲編集の原子性、undo/redo は `process` 層の既存規則に従う。

## エラー

不正な A1 参照や範囲外編集など、`process` 層で発生したエラーは Luau の runtime error としてスクリプト側へ返す。
Rust 側では `LuauError` として受け取る。

## 今後の拡張

候補には以下がある。

- 行・列の挿入削除
- ヘッダー名による列参照
- 列型チェック API
- 保存操作
- ユーザー定義モジュール
- 実行時間・命令数・メモリ量の制限

公開 API は、GUI や Rowly DSL と同様に `process` 境界を越えない形で追加する。

## 正本

この日本語版を Luau 連携仕様の正本とする。英語版を将来追加する場合、差異があるときは日本語版を優先する。
