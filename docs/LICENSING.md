# ライセンス方針

この文書は Rowly リポジトリ内のライセンス適用範囲を整理する日本語ガイドです。法的な利用条件そのものは各ライセンス本文を正本とします。

## Rowly ソースコード

Rowly 本体のソースコードは Apache License 2.0 で公開します。リポジトリ直下の `LICENSE` が全文です。

`Cargo.toml` の package metadata も `Apache-2.0` とします。

## ソースファイルの SPDX 表記

現時点では、Rowly が新規作成する各ソースファイルへ SPDX ヘッダーを一律追加しません。リポジトリ直下の `LICENSE` と package metadata をコードライセンスの基準とします。

個別ファイルを他プロジェクトから取り込む場合や、別ライセンスの表示が必要な生成物・第三者コードを追加する場合は、そのファイルの由来とライセンス表示を保持します。将来 SPDX ヘッダーを採用する場合も、Apache-2.0 に追加条件を設ける目的では使いません。

## 第三者コンポーネント

Rust crates、Python runtime、openpyxl、PyInstaller その他の第三者コンポーネントには、それぞれのライセンスが適用されます。配布物へ同梱する第三者ライセンスは、配布ビルドが生成する NOTICE / license files でも確認できるようにします。

Rowly の Apache-2.0 は第三者ソフトウェアのライセンスを上書きしません。

## 文書

コード以外の文書について、個別文書に別ライセンスを明示した場合はその表示を適用します。Rowly Design Principles のライセンスは別 Issue で扱い、コードの Apache-2.0 と混同しません。

## 商標

Apache-2.0 は Rowly の名称・ロゴ等の商標利用を包括的に許可するものではありません。商標ポリシーはこの Issue の対象外です。

## GUI の日本語フォント

Rowly GUI は Noto Sans CJK JP Regular を組み込みます。フォントは SIL Open Font License 1.1 で提供されます。出典は `assets/fonts/README.md`、ライセンス本文は `assets/fonts/OFL.txt`、著作権表示は `assets/fonts/COPYRIGHT.txt` に記録しています。配布ビルドは `licenses/NotoSansCJKJP-OFL.txt` と `licenses/NotoSansCJKJP-COPYRIGHT.txt` を作成します。GUI 実行ファイルを別途配布する場合は、この二つのファイルを実行ファイルと同じ配布物に同梱してください。
現在の配布ビルドは CLI と Excel バックエンドを収録し、GUI 実行ファイルを生成しません。GUI 実行ファイルにはフォント本体が埋め込まれています。GUI またはフォントを別途配布する場合は、フォント本体を含む実行ファイルまたはフォントファイルと、上記のライセンス本文と著作権表示を一緒に配布してください。
