# 自己完結配布仕様

この日本語文書を Excel backend を含む Rowly 配布物の正本とする。

## 配布単位

正式配布では、少なくとも次を同じ配布ディレクトリに置く。

```text
rowly(.exe)
rowly-excel-bridge(.exe)
rowly-distribution.json
THIRD_PARTY_NOTICES.md
licenses/
```

`rowly-excel-bridge` は PyInstaller の one-file executable とし、Python runtime、openpyxl、et-xmlfile、Rowly の Excel bridge code を内包する。ユーザー環境の Python / pip / openpyxl を通常動作の前提にしない。

## 固定バージョン

配布相当ビルドでは Python 3.12.14 を使用する。Excel runtime dependency は `python/requirements.txt`、packager は `python/build-requirements.txt` に完全固定する。

現行値:

- Python 3.12.14
- openpyxl 3.1.5
- et-xmlfile 2.0.0
- PyInstaller 6.22.3

バージョンを更新する場合は、Windows / Ubuntu の packaged XLSX import CI を同じ変更で通す。

## runtime 解決

`ROWLY_PYTHON` が設定されている場合は開発・デバッグ override として Python source bridge を起動する。

override がない場合、Rowly は自分自身の executable と同じディレクトリの `rowly-excel-bridge` を探す。release build で helper が欠落している場合は配布破損として失敗し、system Python を探さない。

debug build だけは source checkout での開発用として system `python` / `python3` fallback を許可する。この fallback はユーザー向け配布仕様ではない。

## ビルド

`tools/build_distribution.py` は release Rowly と PyInstaller helper を生成し、同一出力ディレクトリへ配置する。正式配布相当ビルドは固定 Python version を要求する。

生成時に `rowly-distribution.json` へ実際の Python / openpyxl / et-xmlfile / PyInstaller version を記録する。取得できる第三者ライセンス本文を `licenses/` へコピーし、`THIRD_PARTY_NOTICES.md` を生成する。

## CI の受け入れ条件

CI は source tree 上の Python bridge テストだけでは完了としない。

1. release Rowly をビルドする。
2. Python runtime / openpyxl を内包する helper を生成する。
3. 配布ディレクトリを組み立てる。
4. system Python を使えない子環境を作る。
5. packaged Rowly で実物 XLSX fixture を import する。
6. UTF-8 CSV 正本が生成されることを確認する。
7. 生成 CSV を `CsvDocument::open` で再読込し、期待値を検証する。
8. packaged Rowly だけで CSV → XLSX → CSV round trip も検証する。
9. Windows / Ubuntu の両方で同じ検証を行う。

fixture は日本語、補助漢字、絵文字、`001`、`=1+1`、空セル、カンマ、改行、Boolean、日本語・空白入りシート名を含む。

## 保証の境界

Excel は交換形式であり、XLSX を第二の正本にはしない。書式、罫線、色、merged cell、数式計算を Rowly の canonical model に導入しない。

PyInstaller helper は Python runtime を自己完結させるための実装詳細であり、ユーザー向け Python scripting environment ではない。
