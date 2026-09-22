# Excel 連携仕様

## 目的

Excel は Rowly の入出力経路として扱う。CSV が正本であり、Excel ファイルを開いたまま第二の正本として保持しない。

Excel の読書き実装は Python / openpyxl を使用する。Rust 側の `excel_python` アダプタが process 境界と Python bridge の間を仲介する。

## 依存境界

```text
CsvDocument
    |
    | rows / create
    v
excel_python (Rust)
    |
    | JSON stdin/stdout
    v
python/excel_bridge.py
    |
    v
openpyxl / .xlsx
```

Python bridge は CSV codec や `data` 層へ直接アクセスしない。
export 元のデータは `CsvDocument` から受け取り、import 結果は `CsvDocument::create` で UTF-8 CSV 正本として作成する。

## Export

`excel_python::export_document(document, output_path, sheet_name)` を使用する。

- 対象は1ワークシート。
- Rowly の各 CSV セルは Excel 側でも文字列セルとして書き込む。
- `001` を数値 `1` にしたり、`=1+1` を数式として解釈したりしない。
- CSV の ragged row は、存在するセルだけを Excel へ書き込む。
- 書式、色、列幅などは正本情報として生成しない。

## Import

`excel_python::import_workbook(input_path, csv_path, sheet_name)` を使用する。

- `sheet_name` 指定時はそのワークシートだけを読む。
- 未指定時は workbook の active sheet を読む。
- 読み込んだセル値は CSV の canonical 文字列へ変換する。
- Boolean は `true` / `false` へ変換する。
- その他の値は Python の文字列表現へ変換する。
- 行末の空セルは切り落とし、ragged row として扱う。
- workbook 末尾の完全空行は切り落とす。
- import 結果は指定した CSV パスへ UTF-8 で作成し、clean な `CsvDocument` として返す。

## 数式

import は `data_only=False` で workbook を読み込む。
数式セルは計算結果ではなく式文字列を CSV へ取り込む。

export は Rowly の CSV 文字列を数式として再解釈せず、Excel の文字列セルとして書き込む。

## merged cell

Rowly のデータモデルに merged cell は存在しない。
Excel import 時に merged cell 固有の構造を Rowly 側へ持ち込まない。openpyxl が返すセル値を平坦な行列として取り込む。

## CLI

ヘッドレス CLI は既存の Rust Excel アダプタを呼び出すだけで、Python bridge や CSV codec を直接扱わない。

```text
rowly excel import <xlsx-path> <csv-path> [sheet-name]
rowly excel export <csv-path> <xlsx-path> [sheet-name]
```

- import のシート名省略時は workbook の active sheet を使用する。
- export のシート名省略時は `Sheet1` を使用する。
- import 成功時は作成した CSV のパス、行数、列数を表示する。
- export 成功時は出力 workbook、シート名、行数、列数を表示する。
- 引数個数や subcommand が不正な場合は usage を表示して終了コード 2 とする。
- CSV open、Python 起動、bridge、protocol の失敗はエラーを表示して失敗終了する。
- 従来の `rowly <csv-path>` による CSV 情報表示は互換性のため維持する。

## Python 実行環境

既定の Python executable は Windows では `python`、それ以外では `python3`。
環境変数 `ROWLY_PYTHON` が設定されている場合はその executable を使用する。空白を含むパスも実行ファイル名として渡し、シェルコマンドとして解釈しない。

Rust / Python 間の JSON 通信は UTF-8 とする。bridge の子プロセスに `PYTHONUTF8=1` / `PYTHONIOENCODING=utf-8` を設定し、OS のコードページや親プロセスの Python 標準ストリーム設定に依存しない。親プロセスの環境は変更しない。

Python 依存は `python/requirements.txt` を正本とする。
現時点では openpyxl 3 系を使用する。

Python executable が起動できない場合、bridge が失敗した場合、JSON protocol が不正な場合は Rust 側で明示エラーとして返す。

Windows / Ubuntu 両方で実際の CLI と Excel 往復を継続検証する。対象範囲と実行方法は [CI.md](CI.md) を参照する。

## 今回扱わないもの

- 複数シートの同時 import
- 複数 CSV から1 workbook を構成する export
- Excel 書式の保存
- 列幅、行高、色、フォント、罫線の保存
- 数式の計算
- Excel を正本として継続編集する機能
- merged cell を Rowly の構造として保持する機能

## 正本

この日本語版を Excel 連携仕様の正本とする。英語版を追加する場合も、差異があるときは日本語版を優先する。
