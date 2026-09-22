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
- `Rowly.begin_transaction()` — process transaction を開始する。
- `Rowly.commit_transaction()` — transaction 内の編集を1つの履歴操作として確定する。
- `Rowly.rollback_transaction()` — transaction 内の編集を履歴へ残さず元へ戻す。

例:

```luau
assert(Rowly.cell("A2") == "Alice")
Rowly.set_cell("B2", "42")
Rowly.set_range("A3:B3", "updated")

Rowly.begin_transaction()
Rowly.set_cell("B2", "42")
Rowly.set_cell("B3", "99")
Rowly.commit_transaction()
```

## Transaction

Luau の transaction 制御は独自実装を持たず、すべて `process::CsvDocument` の transaction API を直接呼び出す。
commit 後は transaction 全体が1回の undo/redo 単位になり、rollback は履歴を追加せず transaction 内の編集を逆順に復元する。
transaction のネストや active transaction がない状態での commit / rollback は process 層のエラーとして Luau runtime error へ変換する。

スクリプト開始時に transaction が存在しなかった場合、そのスクリプトが開始した transaction を未commitのまま runtime error・キャンセル・実行時間超過・interrupt上限超過などで終了したときは、Luau アダプタが終了処理で rollback する。これにより失敗したスクリプトが active transaction を process 層へ取り残さない。
既に外部で開始されていた transaction は自動 rollback の対象にしない。また一度 commit 済みの変更を、その後のスクリプトエラーを理由に巻き戻すこともしない。

## データ規則

CSV の正本データは文字列である。
Luau からセルへ書き込む値も現時点では文字列だけを受け付ける。
Lua/Luau のテーブル、関数、userdata などを暗黙に CSV 文字列へ変換して保存してはならない。

A1 参照の解釈、範囲編集の原子性、履歴記録は `process` 層の既存規則に従う。Luau には `Rowly.undo` / `Rowly.redo` を公開しない。Luau から行った編集は通常の document history へ記録され、スクリプト終了後に GUI / process 側から Undo / Redo できる。

## エラー

不正な A1 参照や範囲外編集など、`process` 層で発生したエラーは Luau の runtime error としてスクリプト側へ返す。
Rust 側では `LuauError` として受け取る。

## 実行制限

Luau ユーザースクリプトは必ず実行制限付きで起動する。
通常の `execute` は次の既定値を使用する。

- 最大実行時間: 5秒
- 最大 VM interrupt 回数: 1,000,000回
- 最大 Luau VM メモリ: 64 MiB

実行時間または interrupt 回数を超えた場合は interrupt callback からエラーを返して Luau VM を停止する。
無限ループを含む Luau コードも VM の safepoint で停止対象になる。
メモリ上限を超える割り当ては Luau VM の allocator で拒否する。

テスト、将来の設定画面、用途別プロファイルでは `execute_with_limits` と `LuauLimits` を使って上限を明示的に差し替えられる。
interrupt 回数は Luau 命令数そのものではなく、VM が interrupt callback を呼んだ回数として扱う。

実行制限による停止は `LuauError::Limit`、メモリ上限による停止は `LuauError::Memory` として Rust 側へ返す。

## 外部からのキャンセル

実行ごとに `LuauCancellationToken::new()` でトークンを作り、実行側へ参照を渡す。GUI や上位ランタイムはその clone を保持し、`cancel()` で停止を要求できる。トークンはスレッド間で共有できるが、Luau VM と `CsvDocument` を停止側へ共有する必要はない。

Rust 側の入口は次の2つとする。いずれも同期実行であり、実行スレッドの配置は呼び出し側が管理する。

```rust
use rowly::luau::{
    LuauCancellationToken, LuauError, LuauLimits,
    execute_with_cancellation, execute_with_limits_and_cancellation,
};

// document: &mut CsvDocument、script: &str を用意する。
let cancellation = LuauCancellationToken::new();
let stop_handle = cancellation.clone(); // GUI / 上位ランタイム側で保持する。
let result = execute_with_cancellation(document, script, &cancellation);
// 実行中は別スレッドの stop_handle.cancel() で停止を要求できる。
if matches!(result, Err(LuauError::Cancelled)) {
    // キャンセル終了として扱う。
}
```

`execute_with_cancellation` は既定の実行制限を使用する。上限も指定する場合は `execute_with_limits_and_cancellation(document, script, limits, &cancellation)` を使用する。従来の `execute` / `execute_with_limits` のシグネチャと既定制限は変えない。

トークンの停止状態は解除できない。`cancel()` は繰り返し呼べるが、停止の完了を待つ関数ではない。完了は実行関数の戻り値で判断する。キャンセル済みトークンを再利用すると実行前に `LuauError::Cancelled` になり、新しいトークンで開始した別実行には影響しない。

### 停止の確認位置

停止は実行前、Luau VM の interrupt callback、各 `Rowly.*` API の入口、実行結果を返す前に確認する。停止要求を確認した API 呼び出しは process 操作へ進まない。`pcall` / `xpcall` が停止エラーを捕捉した場合でも、Rust 側では成功にせずキャンセルとして扱い、必要な rollback を行う。

OS スレッドの危険な強制終了や panic は使わない。停止応答は VM の safepoint に依存し、実行中の長い Rust / C 関数、コンパイル、単一の範囲編集そのものを途中で強制終了するものではない。その処理が戻った後の確認位置で停止するため、固定のミリ秒以内に停止する保証はない。

### キャンセルと履歴

スクリプト開始時に transaction がなければ、終了時に残った未確定 transaction を既存の process rollback で戻す。値・dirty 状態・既存 undo/redo 履歴を元に戻し、キャンセル自体で履歴を増やさない。

commit 済み編集と transaction 外で既に完了した編集は残る。実行全体の自動 transaction 化ではない。実行前から外部で開始されていた transaction は呼び出し側の管理下に残し、呼び出し側が rollback / commit を決める。開始前のキャンセルでは CSV や外部 transaction を変更しない。

通常のキャンセルは `LuauError::Cancelled`、rollback 自体に失敗した場合は従来の `LuauError::Cleanup` で報告する。キャンセル後も同じ `CsvDocument` で別のスクリプトを実行できる。

## 今後の拡張

候補には以下がある。

- 行・列の挿入削除
- ヘッダー名による列参照
- 列型チェック API
- 保存操作
- ユーザー定義モジュール
- GUI からの実行制限設定と停止ボタン

公開 API は、GUI や Rowly DSL と同様に `process` 境界を越えない形で追加する。

## 正本

この日本語版を Luau 連携仕様の正本とする。英語版を将来追加する場合、差異があるときは日本語版を優先する。
