# Rowly

Rowly は、**CSVを正本にしたままExcelを不要にすることを目指す表計算・データ編集アプリケーション**です。

> **CSVを正本とする。**  
> **データは結合しない。見た目だけをまとめる。**

Excel互換クローンではありません。数式・表示・自動化・プロジェクト状態をCSV値そのものへ混ぜず、普通のCSVとして回収・編集できる状態を守ります。

## 3つのモード

```text
Edit  = 式・規則・割当を編集する
View  = 評価した答えを見る
Code  = CSV正本の生テキストを見る / 直接編集する
```

計算規則はCalculation BindingとしてCSV外に保持し、計算結果をCSVへ反映するときは明示操作にします。詳細は[CSV・計算・表示の設計原則](docs/DATA_MODEL_PRINCIPLES.md)を参照してください。
連続する同じ値のセルはView上でまとめて表示する設計とし、そのためにCSV値を削除・空文字化しません。

## 基本原則

- **CSV is the source of truth**
- 全主要操作をキーボードから実行可能にする
- GUI / Rowly DSL / CLI / Luau は共通のCommand / process境界を使う
- 外部編集と共存し、競合時もRowly側の変更を失わない
- Undo / Redo / 履歴をデータ編集の基本機能とする
- XLSX等はImport / Export用であり、第二の正本にしない
- Project / Macro / File I/Oは安全側を既定とする
- Rowly固有形式だけにデータを閉じ込めない

## 主な機能

- CSVのView / Edit / Code
- 複数CSVの論理テーブル
- Spreadsheet形式の編集、選択、Undo / Redo
- 外部変更検知と競合処理
- Rowly DSL / Luauによる自動化
- `.rwprj` project / `.rowlyx` package
- XLSX Import / Export
- CLI / 対話shell
- Viewer設定、入力規則、Calculation Binding

開発中の機能を含みます。実装状況の正本はソースコード・テスト・対応Issueです。

## 起動

GUI:

```text
cargo run --features gui --bin rowly-gui
```

現在のCLI例:

```text
rowly <csv-path>
rowly excel import input.xlsx output.csv [sheet-name]
rowly excel export input.csv output.xlsx [sheet-name]
```

CLI 1.0では対話shell・JSON出力・Rowly DSL one-shot実行も整備します。

## ソースコードの入口

- GUIは [`src/gui_main.rs`](src/gui_main.rs) から [`src/gui.rs`](src/gui.rs) へ進みます。
- CLIは [`src/main.rs`](src/main.rs) が引数を解析して処理を呼び出します。
- 共通モジュールの一覧は [`src/lib.rs`](src/lib.rs) にあります。
- 表の操作は [`src/process/`](src/process/) を通り、CSVの保持と入出力は [`src/data/`](src/data/) が担当します。
- 依存関係と機能別の読み進め方は [アーキテクチャ文書](docs/ARCHITECTURE.md#コードを読む順序) を参照してください。

## ドキュメント

| 内容 | 文書 |
| --- | --- |
| アーキテクチャ | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| CSV・計算・表示の設計原則 | [docs/DATA_MODEL_PRINCIPLES.md](docs/DATA_MODEL_PRINCIPLES.md) |
| Rowly DSL 文法 | [docs/DSL_REFERENCE.md](docs/DSL_REFERENCE.md) |
| Rowly DSL チュートリアル | [docs/DSL_TUTORIAL.md](docs/DSL_TUTORIAL.md) |
| DSL 標準ライブラリ | [docs/DSL_STANDARD_LIBRARY.md](docs/DSL_STANDARD_LIBRARY.md) |
| DSL 変数・代入 | [docs/DSL_BINDINGS.md](docs/DSL_BINDINGS.md) |
| Luau | [docs/LUAU.md](docs/LUAU.md) |
| Luau sandbox | [docs/LUAU_SANDBOX.md](docs/LUAU_SANDBOX.md) |
| Excel連携 | [docs/EXCEL.md](docs/EXCEL.md) |
| 配布 | [docs/DISTRIBUTION.md](docs/DISTRIBUTION.md) |
| CI | [docs/CI.md](docs/CI.md) |
| ライセンス詳細 | [docs/LICENSING.md](docs/LICENSING.md) |

詳細な未実装仕様・設計判断・実装タスクはGitHub Issuesで管理します。

## ライセンス

Rowly は [Apache License 2.0](LICENSE) で公開します。

## 文書の言語

日本語文書を正本とします。英語版と差異がある場合は日本語版を優先します。
