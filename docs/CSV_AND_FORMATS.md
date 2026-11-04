# CSV基本仕様・文字コード・形式変換・暗号化保存・Rubyスクリプト基盤

CSVの構文と文字コードの契約を固定し、CSV以外の形式はImport / Exportに限り、暗号化保存とRubyスクリプトはP3の方針と採否の判断材料だけを定める設計書です。

- 状態: 提案: Issue本文に基づく設計。利用者未承認
- 確認日: 2026-11-05
- 対象Issue: #8 CSV基本仕様：空値・引用符・改行・制約 / #7 CSV文字コード：UTF-8標準とShift-JIS互換読み込み / #2 CSV / JSON / Markdown / XLSX / SQLite の相互変換と保存形式(親) / #3 暗号化保存モード：CSV正本を暗号化して保持する / #44 mrubyによるRubyユーザースクリプト実行基盤を追加する / #207 不定幅CSVの作成・保存・text出力が列数不一致エラーになる / #218 XLSX exportで32767文字を超えるcellを無警告で切り捨てず位置付きエラーにする
- 関連Issue: #134(DSLのIMPORT / EXPORT命令)、#136(GUIの共通Import / Export)、#240(保存 / Save As / Pack / Exportの永続化契約)
- 読む人: CSV I/O、Import / Export、保存処理を実装する人

## 背景と目的

Rowlyの正本はCSVです。ところが、CSVの構文、文字コード、他形式との関係が文書として固定されていないため、次の問題が起きます。

- 不定幅の行を含むCSVを開けるのに保存できない(#207)。
- Shift-JISを読めるが、利用者への確認とUTF-8への移行方針が未実装である(#7)。
- XLSXしかないImport / Exportに、JSON・Markdown・SQLiteをどう足すか、通常保存とどう区別するかが決まっていない(#2)。
- 暗号化とRubyの要望があり、CSV正本の原則を崩さずに扱う条件が決まっていない(#3、#44)。

この文書は、独自のCSV方言を作らず、CSV以外を外部形式との橋渡しに限る原則を、実装できる契約にします。重複を避けるため、保存の原子性は [PERSISTENCE_CONTRACT.md](PERSISTENCE_CONTRACT.md)、XLSXの詳細は [EXCEL.md](EXCEL.md)、DSLのIMPORT / EXPORTの権限は [DSL_IO_SECURITY.md](DSL_IO_SECURITY.md) を正本とし、ここでは繰り返しません。

## 現行実装との差

確認したファイルは `src/data/csv_io.rs`、`src/data/encoding.rs`、`src/data/table.rs`、`src/process/document.rs`、`src/excel_python.rs` です。

| 項目 | 現行 | 状態 |
| --- | --- | --- |
| 読込 | `read_csv` は `ReaderBuilder::new().has_headers(false).flexible(true)` で、引用符・埋込改行・LF/CRLFを標準のcsv crateで解析し、不定幅の行をそのまま保持する | 実装済み |
| 書込(ファイル) | `write_csv_utf8_if_unchanged` の `WriterBuilder` に `flexible(true)` が無く、行の長さが違うと保存が失敗する | 未修正(#207) |
| 書込(文字列) | `CsvDocument::csv_text` の `WriterBuilder` も同様に失敗する | 未修正(#207) |
| 新規作成 | `CsvDocument::create` は `write_csv_utf8` を通るため、不定幅の行を渡すと失敗する | 未修正(#207) |
| 文字コード判定 | `decode_to_utf8` はUTF-8(BOMを除去)を優先し、失敗時に `chardetng` で判定してShift_JISだけを変換する。他のencodingは `UnsupportedEncoding`、不正なbyte列は `InvalidShiftJis` | 実装済み |
| 保存時の文字コード | `save` / `save_as` は常にUTF-8で書き、`source_encoding` を `Utf8` に更新する。Shift-JIS入力が保存でUTF-8になるテストがある | 実装済み |
| Shift-JIS確認UI | 起動ごとに1回確認する仕組みは `src/gui` にも `src/process` にも無い | 未実装(#7) |
| XLSX | `excel_python::export_document` / `import_workbook` とCLI `rowly excel` がある | 実装済み。詳細は [EXCEL.md](EXCEL.md) |
| 32767文字超 | `src/excel_python.rs` と `python/` に長さ検査が無い。`docs/DSL_IO_SECURITY.md` が位置付きエラー化を方針として記載している | 未実装(#218) |
| JSON / Markdown / SQLite | Import / Exportの実装は無い。`serde_json` は依存にあるが、表形式のadapterは無い | 未実装 |
| 暗号化 | 暗号crateは依存に無く、実装も無い | 未実装(P3) |
| Ruby / mruby | 実装も依存も無い。Luauは `src/luau.rs` に実装済み | 未実装(P3) |

## 設計方針

### 契約(守る不変条件)

1. CSVのraw cell textが唯一の正本です。空値、先頭の0、`=` で始まる文字列を、Rowlyが補正・型変換・再解釈して書き戻すことはしません。
2. 独自のCSV方言を作りません。区切りはカンマ、引用はダブルクォート、引用内の `"` は `""`、レコード区切りはLF/CRLFを読み、書込はLFです(#8)。
3. 空値は許可します。`NOT NULL` はRowlyの検証レイヤーが扱い、制約に違反してもCSVの値は補正も削除もしません。
4. 行ごとの列数は保持します。`Table::new` の `rows` は可変長のままで、矩形化も末尾の空欄補完もしません(#207)。
5. 保存は常にUTF-8です。Shift-JISの標準保存形式は提供せず、元の文字コードやbyte列の維持も要件にしません(#7の追加決定)。
6. CSV以外の形式への出力は保存ではありません。成功してもcanonicalのdirtyは変わりません([PERSISTENCE_CONTRACT.md](PERSISTENCE_CONTRACT.md) の「Exportは保存ではない」)。
7. 出力先の形式が表現できない値は、黙って切り捨てず位置付きエラーにします(Lossless-or-error。[DATA_MODEL_PRINCIPLES.md](DATA_MODEL_PRINCIPLES.md))。

### CSV writer(#207、#8)

`src/data/csv_io.rs` に、読込と対になるwriter設定を1か所(関数名案 `csv_writer_builder`)に集め、`write_csv_utf8_if_unchanged` と `CsvDocument::csv_text` の両方が使います。設定は `terminator(Terminator::Any(b'\n'))` と `flexible(true)` です。`Table::rows` をそのまま `write_record` に渡し、補完はしません。

採用しなかった案:

- 短い行を空欄で補って矩形にする案: 行の長さが変わり、再度開いたときに別のデータになります。Issue #207の対象外にも「自動補完・矩形化」が挙がっています。
- `create` だけを直す案: openした不定幅CSVの `csv_text` / `save` が失敗する追加再現(#207のコメント)を直せません。

### 文字コードと確認(#7)

`decode_to_utf8` の判定順(UTF-8、Shift_JIS、その他はエラー)は変えません。追加するのは、Shift-JISを検出した事実を上位へ伝え、起動ごとに1回だけ利用者へ確認する層です。

```text
ProjectSession
└─ shift_jis_policy: Undecided | ConvertToUtf8 | KeepForNow   // session内だけ。永続化しない
```

- Shift-JISのCSVを最初に開く時点で `Undecided` なら、確認ダイアログを1回表示し、選択をsessionに保持します。以後の同じsessionでは再確認しません。
- `ConvertToUtf8` を選んだ場合は、そのsession中に検出したShift-JIS CSVを、開いた時点または保存時にUTF-8へ変換します。変換は表データを変えず、保存はUTF-8の通常保存と同じ経路です。
- `KeepForNow` を選んだ場合も、読込と編集は可能です。保存すると既存仕様どおりUTF-8になるため、保存前に「保存するとUTF-8になります」と表示します。
- 判定の誤り(UTF-8でもShift-JISでもないbyte列)は、確認を出さず読込エラーにします。誤判定で文字化けしたCSVを正本にしないためです。

確認の状態を `.rwprj` やCSVへ保存しない理由は、Issueが「次回の起動で残っていれば再度1回確認する」と定めているためです。

## 利用者から見た操作

Command IDは案です。既存のIDは `file.open`、`file.save` です([FILE_LIFECYCLE.md](FILE_LIFECYCLE.md))。実装時に [COMMAND_REGISTRY.md](COMMAND_REGISTRY.md) の登録形式へ合わせ、すべてAlt KeyTipsから到達できるようにします。

| 目的 | GUI | CLI / DSL | Command ID案 |
| --- | --- | --- | --- |
| Shift-JIS確認への回答 | ダイアログ(キーボードで選択可能) | CLIは対話なしなら `--encoding-policy convert` などで事前指定し、省略時は読込のみ | `file.encoding.policy` |
| 他形式のImport | Alt → ファイル → Import → 形式 | `rowly import <format> <input> <csv>` / `IMPORT <FORMAT> ... TO CSV ...` | `file.import.xlsx`、`.json`、`.markdown`、`.sqlite` |
| 他形式のExport | Alt → ファイル → Export → 形式 | `rowly export <format> <csv> <output>` / `EXPORT ... TO <FORMAT>` | `file.export.xlsx`、`.json`、`.markdown`、`.sqlite` |
| 暗号化保存(P3) | Alt → ファイル → Save encrypted など | 未定 | `file.save_encrypted` ほか |

失敗時は、失敗した操作、対象のpath、原因(列数、cellの位置、文字数、encoding名)、次にできること(別形式で保存、値の短縮)を表示します。XLSXの32767文字超は、行・列・文字数を位置付きで示し、出力先を作成も変更もしません。

## 永続化と互換性

- CSV本文: 構文の変更はありません。writerの `flexible(true)` は、既に保存できていた矩形のCSVのバイト列を変えません。変わるのは、これまで失敗していた不定幅の保存が成功する点だけです。
- 形式の役割: 通常保存できるのはCSVだけです。JSON・XLSX・SQLite・Markdownは、読み込むとRowlyの同じ論理テーブルになり、編集後はCSVとして保存します。出力は交換成果物であり、backingにはなりません。
- 形式ごとの契約(案。Issueの1.0.0範囲は「実装コストが低くCSV-first境界を崩さないものだけ」で、XLSXは継続、他の形式の採否は未決事項1):

| 形式 | Import | Export | Rowlyへ持ち込まないもの |
| --- | --- | --- | --- |
| XLSX | [EXCEL.md](EXCEL.md) のとおり | 同左。cell値は文字列。32767文字超はエラー | 書式、結合、数式評価 |
| JSON | 配列の配列、またはキーが揃ったオブジェクトの配列を表と解釈する | 同じ2形式のうち、既定は配列の配列 | 入れ子、数値・真偽値の型 |
| Markdown | 先頭の区切り行を持つ表だけを対象にする | `|` と改行をエスケープした表 | 表以外の本文 |
| SQLite | 利用者が選んだ1つのtableを読む | 1つのtableを作る | 型、制約、index、SQL |

- JSONのExportでcellを文字列のまま出す理由は、`001` や `=1+1` を型変換しないためです。型を付けた出力は別のView Export(`view.export.displayed`)に属し、この文書の対象外です。
- project: Importで作ったCSVを `.rwprj` のsourceへ自動登録しません。登録は利用者が別に行います([DSL_IO_SECURITY.md](DSL_IO_SECURITY.md))。

## 暗号化保存モード(#3、P3: 方針と採否の判断材料)

実装は1.0.0の必須ではありません。Issueのコメントは、安全に収まらなければ1.1.0へ延期し、独自暗号や不十分な鍵管理を許容しないと定めています。

守る方針:

- 通常の `Save` は暗号化しません。暗号化は `Save encrypted` を明示した場合だけで、復号しただけでは平文CSVに置き換えません。平文にするのは `Save as plain CSV` だけです。
- 暗号プリミティブは標準方式を使います。Issueの候補は、AEADがAES-GCMまたはXChaCha20-Poly1305、パスワードのKDFがArgon2idです。誤ったパスワードと改ざんは、AEADの検証で復号失敗にします。
- パスワードはファイルへ保存しません。復号後の平文は永続化せず、アプリ終了・ロック時に残しません。
- SQLiteは正本にしません。暗号のversion、KDFパラメータ、salt / nonceなどの補助情報の入れ物として使う案がありますが、必須ではありません(未決事項3)。

採否の判断材料は、次の4点を決められるかです。決められなければ1.1.0へ延期します。

1. 容器の形式(単一の自己記述ファイルか、SQLiteか)とバージョニング。
2. パスワードなし時の鍵の置き場(OSのキーストア連携など)。「透過的に開ける」要件と、鍵を平文ファイルへ置かない要件の両立。
3. 復号済みデータの一時領域の扱い(メモリのみ、クラッシュ時の残存)。
4. 保存失敗時に旧ファイルを保つ原子性。[PERSISTENCE_CONTRACT.md](PERSISTENCE_CONTRACT.md) の一時ファイルと原子的置換を、暗号文にも同じに適用します。

## Rubyユーザースクリプト基盤(#44、P3: 方針と採否の判断材料)

位置付けは、Rowly DSL(正規マクロ言語)、Luau(軽量スクリプト)の次の層です。RubyはRowly DSLの代替ではありません。Issueは実装ランタイムの第一候補をmrubyとしています。

守る方針:

- RubyからCSV codecやdata層へ直接触れさせません。`Rowly` 名前空間のadapterを経て、GUI・DSL・Luauと同じCommand / process境界を通します(初期APIは `Rowly.cell`、`Rowly.set_cell` など既存processの薄い公開)。
- LuauのSandbox([LUAU_SANDBOX.md](LUAU_SANDBOX.md))と同等の権限境界を要求します。標準ライブラリの許可リスト、ファイル・OS・ネットワークの禁止、実行時間・メモリの上限、キャンセルです。
- Ruby固有の表編集は実装しません。Ruby経由の編集は通常のdocument historyに記録されます。Undo / Redoの保証対象外とする範囲は、Issue本文の記述(Luauと同様)に従い、Luauの現行仕様([LUAU.md](LUAU.md))と矛盾しないことを実装時に確認します。
- 利用者に環境構築を求めません。mrubyは配布物へ静的に組み込みます。

採否の判断材料(いずれも未調査):mrubyのRustバインディングの保守状況とライセンス、`mruby` のサンドボックス機構(gem選択、メモリ・命令数の制限の可否)、Windows / Ubuntuでのbuildと配布物のサイズ、Luauと並べて保守する費用。これらが満たされなければ、採用せず見送る選択肢を残します。

## 安全性と失敗時の挙動

- 保存とExportは、出力先と同じdirectoryの一時ファイルへ書き、検証後に原子的に置換します。失敗時は旧ファイルとdirty状態を保ちます(#207の受け入れ条件、[PERSISTENCE_CONTRACT.md](PERSISTENCE_CONTRACT.md))。現行のXLSX bridgeは出力先へ直接保存するため、この点の改修が要ります(#134側の記載と同じ)。
- 形式が表現できない値は、一時ファイルを作る前に検査して位置付きエラーにします(#218)。部分的に出力した結果を残しません。
- Importは入力を検証してから、新しいCSVを作ります。不正な入力(壊れたJSON、表でないMarkdown)は、既存のCSVを変更せずに失敗します。
- 文字コードが判定できない、または変換でエラーが出る場合は、CSVを開かずエラーにします。誤変換した文字列を正本にしません。
- 暗号化(採用する場合)は、復号に失敗したときに平文を一切残さず、旧ファイルを変更しません。

## 実装順序と依存

| 順 | Issue | 最小単位 | 先行条件 |
| --- | --- | --- | --- |
| 1 | #207 | writer設定の共通化。`create`、`save`、`save_as`、`csv_text` で不定幅の往復を確認 | なし |
| 1 | #8 | 既存テストと要件(空値、引用符、埋込改行、LF/CRLF)の対応付け。不足分のテストを追加 | なし。#207と並行可 |
| 2 | #218 | XLSX exportの長さ検査と位置付きエラー | なし。#240の一時ファイル方式と同時が望ましい |
| 2 | #7 | sessionのShift-JIS方針と確認ダイアログ | ProjectSessionの追加。#207と独立で並行可 |
| 3 | #2 / #134 / #136 | JSON・Markdownのadapter(採用する場合)、共通Import / Export Command | #240の契約、[DSL_IO_SECURITY.md](DSL_IO_SECURITY.md) の権限 |
| 4 | #2 | SQLite adapter | 採否の決定(未決事項1) |
| 5 | #3 / #44 | P3の設計確定(コードは書かない) | 1.0後の判断 |

## 受け入れ条件と検証

- #207: 短い行と空欄を含むCSVを作成、open、save、再openして、cell値と各行の列数が一致します。`csv_text()` の出力を再解析しても一致します。保存失敗時に旧ファイルとdirty状態が保たれます。検証は `src/process/document.rs` と `csv_io.rs` の単体テストで行い、回帰テスト、format、Clippy、Windows / Ubuntu CIを通します。
- #8: 空値、`,`・改行・`""` を含むフィールドの往復、LFとCRLFの読込、`NOT NULL` 違反でもCSV値が変わらないことを単体テストで確認します。
- #7: UTF-8(BOMあり・なし)、Shift-JIS、未対応encoding、不正byteを `decode_to_utf8` の単体テストで確認します(一部は既存)。1起動1回の確認は、Shift-JIS CSVを複数含むprojectを開く統合テストと、GUI実操作(キーボードのみ)で確認します。
- #2: 形式ごとに、往復とraw textの保持(`001`、`=1+1`、改行、空欄)を統合テストで確認します。Exportがdirtyを変えないことも確認します。
- #218: 40,000文字のcellを含むCSVをXLSXへ出力すると、位置付きエラーになり、出力先が作られないことを確認します。
- #3 / #44: 受け入れ条件は設計の確定です。誤パスワード・改ざん・保存失敗の期待結果(#3)と、採用候補の実行制限の検証方法(#44)を、この文書の判断材料に基づき決めた時点で完了です。

## 未決事項

1. JSON・Markdown・SQLiteのうち1.0.0へ含める形式。選択肢: (a) 全形式、(b) JSON・MarkdownのExportのみ、(c) XLSXのみ。推奨は(b)です。理由は、Issueが「実装コストが低くCSV-first境界を崩さないものだけ」と定めており、JSON・Markdownは外部依存が少なく、SQLiteはtable選択・型の扱いという設計課題が大きいためです。
2. JSON Importが受け付ける構造。選択肢: (a) 配列の配列のみ、(b) 加えてオブジェクトの配列。推奨は(b)です。理由は、見出し付きの一般的なJSONを読めるためです。ただし、キーの集合が行ごとに違う場合の列順を決める必要があります。
3. 暗号化の容器にSQLiteを使うか。選択肢: (a) 単一の自己記述ファイル、(b) SQLite。推奨は(a)です。理由は、SQLiteを必須にするとSQLite adapterの採否に暗号化が依存するためです。
4. Shift-JISで `ConvertToUtf8` を選んだときの変換のタイミング。選択肢: (a) 開いた時点でファイルを書き換える、(b) 保存時まで書き換えない。推奨は(b)です。理由は、確認だけで利用者のファイルが変わるのを避けられるためです。
5. CLIで対話なしにShift-JISを扱う既定。選択肢: (a) 読込のみ(保存時にUTF-8)、(b) 指定がなければエラー。推奨は(a)です。理由は、Issueが「変換を断っても読み込みは可能」と定めているためです。
