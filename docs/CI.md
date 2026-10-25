# Windows / Ubuntu の継続検証

この日本語文書を Issue #61 の CI 仕様の正本とする。

## 対象と位置付け

Windows を公式対応 OS、Ubuntu を準対応 OS とする。CI の両 OS 実行は、Ubuntu を Windows と同一の公式サポートへ格上げするものではない。

`.github/workflows/ci.yml` は既存の `push` / `pull_request` を対象とし、Rust 1.98.1 と Python 3.12.10を使って `windows-latest` / `ubuntu-latest` の両方で検証します。Rustとrustfmt・Clippyは、ローカルの `rust-toolchain.toml` とCIで同じバージョンに固定しています。ランナーの具体的なOSイメージは各実行のログで確認してください。

## ジョブ構成

`rust-matrix` は OS ごとに次の処理を実行する。

```text
固定版 Python runtime / openpyxl / PyInstaller 依存の準備
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --all-targets --all-features
cargo test --all-targets --all-features
release Rowly + bundled Excel bridge の配布相当成果物をビルド
packaged Rowly だけで実 XLSX fixture を import / export
生成 CSV を CsvDocument で再 open して値を検証
```

`fail-fast: false` により、片方のOSが失敗しても他方の検証を打ち切りません。各OSジョブには35分の上限を設定しています。

従来のチェック名 `rust` は集約ジョブとして維持する。`always()` で両 OS の結果を評価し、matrix 全体が `success` のときだけ成功する。失敗・キャンセル・スキップを成功とみなさない。既存のブランチ保護設定自体はこの変更で更新しない。

ワークフローの権限は `contents: read` に限定し、checkout 後の Git 設定に認証情報を保持しない。

## Alt KeyTips 全到達性ゲート

Rowlyでは、**GUIから実行可能な全Commandが最低でもAlt KeyTipsから到達できること**をUIの絶対条件とする。

この条件は #200 でcoverage validator / testを実装し、#201 でWindows / Ubuntu CIの専用gateとして組み込む。

CIでは通常の `cargo test --all-targets --all-features` に含まれていることだけに依存せず、KeyTips coverageを独立したstepとして実行する。

次のいずれかを検出した場合、CIを失敗させる。

- GUI CommandにAlt KeyTips pathが無い
- KeyTips path途中に到達不能nodeがある
- 同一階層でaccess keyが衝突する
- mode / contextで有効なCommandへ、そのcontextからAltで到達できない
- dialog / secondary UI内にkeyboard-onlyで完了できない主要actionがある
- shortcut-only / Command Palette-only / mouse-onlyのGUI Commandが存在する

このgateは `windows-latest` / `ubuntu-latest` の両方で実行し、どちらか一方でも失敗した場合は集約required check `rust` を失敗させる。

**専用shortcutの有無は合否条件ではない。Alt KeyTipsから全GUI Commandへ到達できることが最低条件である。**

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

## 配布相当 Excel backend の実動検証

CI は `tools/build_distribution.py` で release Rowly と PyInstaller 製 `rowly-excel-bridge` を同じディレクトリへ組み立てる。bridge には Python runtime / openpyxl / et-xmlfile が内包される。

その後 `ROWLY_PYTHON`、`PYTHONHOME`、`PYTHONPATH` を子環境から除外し、さらに `PATH` を配布ディレクトリだけへ制限して `tools/verify_packaged_excel.py` を実行する。system Python への fallback が残っていればこの段階で成功できない。

リポジトリに実物の `tests/fixtures/excel/rowly_import_fixture.xlsx` を保持し、日本語シート名、補助漢字、絵文字、先頭ゼロ、数式に見える文字列、空セル、カンマ、改行、Boolean を import する。生成した `imported.csv` と packaged Rowly による CSV → XLSX → CSV の `roundtrip.csv` は、`tests/packaged_excel_fixture.rs` で `CsvDocument::open` し、canonical 文字列を確認する。

配布物そのものも CI artifact として保存する。この検証は「runner に Python が入っているから Excel が動く」ことを受け入れ条件にしない。

## ローカル検証

Python 3 と Rust stable を準備し、`python -m pip install -r python/requirements.txt` の後に上記の Cargo コマンドを実行する。Python の名前や場所が異なる環境では、シェル側で `ROWLY_PYTHON` に実行ファイルのパスを指定する。

## 保証の境界

現段階ではヘッドレス検証であり、GUI の起動・描画・操作性・配布インストーラーを確認したという意味ではない。egui / eframe 導入後も GUI を全ターゲット・全 feature のビルド対象に含め、必要なシステム依存を両 OS の CI へ追加する。GUI の実操作確認は別の受け入れ検証として行う。macOS CI は対象外。

## 参考

- [GitHub Actions の matrix](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/run-job-variations)
- [setup-python の入出力](https://github.com/actions/setup-python/blob/v5/action.yml)
- [Python 3.12 の環境変数](https://docs.python.org/3.12/using/cmdline.html#environment-variables)
