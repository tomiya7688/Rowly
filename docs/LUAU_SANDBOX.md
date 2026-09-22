# Luau 1.0 サンドボックス仕様

この日本語文書を Issue #32 の権限境界の正本とする。実行 API・transaction・キャンセルの仕様は [LUAU.md](LUAU.md) を参照する。

## 目的と適用範囲

Luau スクリプトへ公開するホスト機能は、明示登録した `Rowly.*` API だけとする。任意ファイルの読み書き、ネットワーク接続、OS コマンド・外部プロセス起動、ネイティブライブラリ読み込みは提供しない。1.0 ではサンドボックス解除用の設定、環境変数、スクリプト API を設けない。

`execute`、`execute_with_limits`、`execute_with_cancellation`、`execute_with_limits_and_cancellation` の全入口で同じサンドボックスを使用する。公開 API は VM を受け取らず、実行ごとに新しい VM を作成する。テスト内部のホスト関数注入経路は公開しない。

## 初期化

`src/luau/sandbox.rs` が `Lua::new_with` で標準ライブラリを明示選択し、基本ライブラリや mlua が追加したグローバルも許可リストに限定する。`ALL` / `ALL_SAFE` による一括公開には依存しない。

既存の時間・interrupt 回数・VM メモリ上限を設定し、process API の関数を登録した後、`Rowly` テーブルを read-only にする。さらに `Lua::sandbox(true)` で標準ライブラリ、組み込みメタテーブル、共有グローバルを保護してからユーザーソースを実行する。

Rust 側の読み込みも `ChunkMode::Text` に限定する。事前コンパイル済みの任意バイトコードを受け入れる経路を提供しない。

## 許可する標準機能

公開するライブラリは `string`、`table`、`math`、`utf8`、`bit32`、`buffer`、`vector`、`coroutine` とする。

基本グローバルの許可リストは次のとおり。ランタイムに存在するものだけを公開し、この一覧にない関数が依存更新で追加されても自動公開しない。

```text
_G  _VERSION
assert  error  gcinfo  getmetatable  ipairs  next  pairs
pcall  xpcall  rawequal  rawget  rawlen  rawset  select
setmetatable  tonumber  tostring  type  typeof  unpack
```

変数、条件分岐、関数、テーブル、メタテーブル、coroutine を使う通常の Luau 処理は利用できる。`gcinfo` は VM メモリ使用量の参照のみで、GC 制御を公開するものではない。

## 公開しないもの

`io`、`os`、`package`、`require`、`dofile`、`loadfile`、`load`、`loadstring`、`debug`、`getfenv`、`setfenv`、`ffi`、`jit` は公開しない。`socket`、HTTP、ファイルシステム、シェル、プロセスのホスト API も登録しない。

`collectgarbage`、`print`、`warn`、`newproxy` は今回の許可リストに含めない。従来の暗黙公開に依存するスクリプトに対する 1.0 前の互換性変更となる。将来のログ出力は、必要になった時点で明示的な Rowly API として別途設計する。

`Rowly` はセル読み取り、セル・範囲書き込み、行列数、begin / commit / rollback の8関数に限定する。`Rowly.undo` / `redo`、任意保存、Python Excel bridge、キャンセル解除、VM 制限変更などへの抜け道は追加しない。

## 保護と実行間の分離

`Rowly` や標準ライブラリの既存テーブルは、通常代入・`rawset`・メタテーブル変更で改変できない。ユーザーが作成した通常のテーブルは編集可能なままとする。

Luau のローカル実行環境へのグローバル変数代入は許可する。例えば `Rowly = {}` と書くことはその実行の名前を隠すだけであり、元の API テーブルを書き換えたり、新しいホスト権限を得たりする操作ではない。次回の実行には変数や上書きが残らない。

## エラー・キャンセル・履歴

禁止 API の呼び出しは通常の Luau 実行エラーとして扱う。`pcall` で捕捉しても権限が追加されることはない。

スクリプト自身が開始した未確定 transaction を残して失敗した場合は、既存の process rollback で復元する。確定済み編集、transaction 外の完了済み編集、外部開始済み transaction の扱いは [LUAU.md](LUAU.md) の既存方針を変えない。

実行時間・interrupt 回数・VM メモリ上限と外部キャンセルはサンドボックスと併用する。停止トークン、既定上限、停止応答の保証範囲も変更しない。これは OS プロセス隔離ではなく、組み込み VM と明示 API による能力制限である。Luau / mlua の未知の実装脆弱性まで存在しないと保証するものではない。

## 回帰検証

全公開入口のグローバルと Rowly API の列挙、禁止 API の不在と呼び出し失敗、テーブルの read-only 保護、通常の標準機能と process 編集、実行間の分離、失敗時 rollback、メモリ・時間上限を専用テストで確認する。既存の外部キャンセル・interrupt 上限・履歴テストも同じ CI で継続実行する。

## 参考

- [Luau のサンドボックス設計](https://luau.org/sandbox/)
- [Luau 標準ライブラリ](https://luau.org/library/)
- [mlua の Lua::sandbox API](https://mlua.rs/mlua/struct.Lua.html#method.sandbox)
