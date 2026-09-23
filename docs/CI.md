# Windows / Ubuntu の継続検証

この日本語文書を Issue #61 の CI 仕様の正本とする。

## 対象と位置付け

Windows を公式対応 OS、Ubuntu を準対応 OS とする。CI の両 OS 実行は、Ubuntu を Windows と同一の公式サポートへ格上げするものではない。

`.github/workflows/ci.yml` は既存の `push` / `pull_request` を対象とし、Rust stable と Python 3.12 を使って `windows-latest` / `ubuntu-latest` の両方で検証する。ランナーの具体的な OS イメージや Rust のパッチバージョンは固定せず、各実行のログで確認する。

## ジョブ構成

`rust-matrix` は OS ごとに次の処理を実行する。

```text
Python / openpyxl 依存の準備
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --all-targets --all-features
cargo test --all-targets --all-features
```

`fail-fast: false` により、片方の OS が失敗しても他方の検証を打ち切らない。各 OS ジョブには20分の上限を設定する。

従来のチェック名 `rust` は集約ジョブとして維持する。`always()` で両 OS の結果を評価し、matrix 全体が `success` のときだけ成功する。失敗・キャンセル・スキップを成功とみなさない。既存のブランチ保護設定自体はこの変更で更新しない。

ワークフローの権限は `contents: read` に限定し、checkout 後の Git 設定に認証情報を保持しない。

## Python と文字コード

CI の Rust テストには、`actions/setup-python` の `python-path` 出力を `ROWLY_PYTHON` として渡す。openpyxl を導入した Python と、Rust が起動する Python を一致させる。

通常実行の既定値は Windows では `python`、それ以外では `python3`。`ROWLY_PYTHON` による明示指定を優先する。パスに空白があってもシェル文字列に連結せず、実行ファイルのパスとして渡す。

Excel bridge の JSON は UTF-8 とする。Rust は bridge の子プロセスだけに `PYTHONUTF8=1` / `PYTHONIOENCODING=utf-8` を設定し、親のコードページや標準ストリーム設定を引き継がない。CSV データの意味・型変換規則は変更しない。

## 回帰テスト

既存の core / process / DSL / Luau / Excel / CLI のテストに加え、`tests/cli_cross_platform.rs` で実際の CLI バイナリを起動して確認する。

- 日本語・空白入りの作業ディレクトリ、相対ファイルパス、シート名
- CRLF CSV の読み込みと、閲覧・export 時に入力ファイルを変更しないこと
- 日本語・補助漢字・絵文字・引用符内カンマ／改行・先頭ゼロ・式に見える文字列の Excel 往復
- import 結果の UTF-8 / LF 保存
- 既定 export シートと active sheet import
- `ROWLY_PYTHON` の起動失敗と、出力ファイルを生成しないこと
- 不正引数の終了コード2

通信テストでは CLI 側へ意図的に `PYTHONUTF8=0` / `PYTHONIOENCODING=ascii` を渡す。bridge 側の UTF-8 固定を削除した場合にも偶然テストが成功しないようにする。環境変数は子プロセスの `Command` に設定し、テスト全体の環境を書き換えない。

## ローカル検証

Python 3 と Rust stable を準備し、`python -m pip install -r python/requirements.txt` の後に上記の Cargo コマンドを実行する。Python の名前や場所が異なる環境では、シェル側で `ROWLY_PYTHON` に実行ファイルのパスを指定する。

## 保証の境界

現段階ではヘッドレス検証であり、GUI の起動・描画・操作性・配布インストーラーを確認したという意味ではない。egui / eframe 導入後も GUI を全ターゲット・全 feature のビルド対象に含め、必要なシステム依存を両 OS の CI へ追加する。GUI の実操作確認は別の受け入れ検証として行う。macOS CI は対象外。

## 参考

- [GitHub Actions の matrix](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/run-job-variations)
- [setup-python の入出力](https://github.com/actions/setup-python/blob/v5/action.yml)
- [Python 3.12 の環境変数](https://docs.python.org/3.12/using/cmdline.html#environment-variables)
