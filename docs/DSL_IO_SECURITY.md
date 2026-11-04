# DSL / Project / File I/O セキュリティ設計

Rowly DSLが外部ファイル・project構造・Import/Exportを操作できるようにする際の、capability型・default deny・path scope・trustを定義する設計書です。

- 状態: 提案: Issue本文に基づく設計。利用者未承認
- 確認日: 2026-11-05
- 対象Issue: #135 DSL I/O capabilityのdefault deny・path scope強制 / #133 Local File I/O命令 / #134 IMPORT・EXPORT命令とadapter呼出し / #130 DSLからのProject I/O Command / #136 GUI各モードの共通Import/Export / #137 1.0前セキュリティゲート / #5 親Issue(project形式・マクロセキュリティ)
- 関連Issue: #241(本文を取得できず未確認)、#219 rowlyx pack衝突(本文を取得できず未確認)、#218 XLSX 32767文字
- 読む人: capability gate、Local File I/O、Import/Export、Project I/O、security testを実装する人

## 背景と目的

DSLにFile I/O・Project I/O・Import/Exportを追加すると、projectを開く、他人のmacroを実行する、といった操作が利用者のファイルを書き換えられるようになります。権限の境界が実装ごとにばらつくと、projectを開いただけで外部ファイルを読み書きされる事故が起きます。

この設計書は、全I/O経路が必ず通る1つの権限境界を定め、1.0リリース前にその境界を攻撃用fixtureで検証する方法を定めます。Luau側の権限境界は [LUAU_SANDBOX.md](LUAU_SANDBOX.md) が正本です。Luauは `Rowly.*` の8関数以外にファイル・network・processを持たないため、この文書の対象外です。この文書はLuauへI/Oを追加しません。

## 現行実装との差

確認したファイルは `src/rowly_dsl/ast.rs`・`src/rowly_dsl/runtime/`、`src/project_init.rs`・`src/project_init/script_includes.rs`、`src/project_session.rs`、`src/rowlyx.rs`、`src/excel_python.rs`、`src/data/csv_io.rs`、`src/main.rs`、`src/gui_commands.rs`、`python/excel_bridge.py` です。

| 項目 | 現行 |
| --- | --- |
| DSL文 | `ast::Statement` はIf/For/Declare/Assign/Return/Call/MethodCall/SetField/SetRangeValue/SetCalculation*/ValidateColumnType/CheckJapanese/SetValidationRule だけです。I/O文は未実装です。 |
| init DSL | `project_init::apply_safe_init` が、`init.rly` から `INCLUDE GENERATED` / `INCLUDE USER` で参照された設定文だけを適用し、任意DSL・macro・filesystem操作を拒否します(`parse_init_includes`)。ConfigOnlyに相当する実装済みの入口です。 |
| capability・path scope・trust store | 型・コードとも未実装です。 |
| `rowly dsl run` / `rowly shell` | `src/main.rs` に存在しません(確認できる subcommand は `excel import/export` のみ)。 |
| Excel adapter | `excel_python::export_document` / `import_workbook` が実装済みです。bridgeは `workbook.save(output_path)` で出力先へ直接書き、既存ファイルの上書き確認も一時ファイル経由の置換もしません。 |
| rowlyx | `pack_project` は絶対参照とproject外へ解決されるsymlink参照を拒否し、一時archive経由で置換します。`RowlyxArchive::open` は不正path・重複entry・symlinkを検査し、`safe_relative_path` が相対pathを検証します。`extract_to` は展開先へ直接書き出し、展開先全体の原子的な切替は確認できませんでした。 |
| ProjectSession | `ProjectSession::save` は `.rowlyx` を `pack_project` で再packします。Project Command APIは #129 で未実装です。 |
| GUI | `src/gui_commands.rs` と `src/gui.rs` にImport/Exportのcommand・メニューは見当たりません。 |
| 安全な書込み | `src/data/csv_io.rs` は `create_new` による一時ファイルと `replace_file`(rename)を持ちます。Local File I/Oが再利用できる手本です。 |

## 設計方針

### 契約(不変条件)

1. 権限は既定で拒否します。許可は実行単位に明示された `CapabilitySet` だけです。
2. 実行時の検査が最終境界です。preflightは利用者向けの事前表示であり、runtime検査の代わりにしません。
3. 検査が曖昧、または検査自体が失敗した場合は拒否します。
4. projectファイルと `.rowlyx` は、自分自身をtrustedと宣言できません。
5. network・process実行は1.0のDSLへ公開しません。Excel adapterが内部で起動するhelperは、DSLから実行ファイル・引数・環境変数を指定できません。`ROWLY_PYTHON` は開発用overrideであり、DSLから設定しません。
6. CSVのraw cell textを正本に保ちます。I/O命令の副作用でCSV本文・表示設定を暗黙に書き換えません([DATA_MODEL_PRINCIPLES.md](DATA_MODEL_PRINCIPLES.md))。

### データモデル案

```text
enum Capability { Config, FileRead(PathScopes), FileWrite(PathScopes), ProjectWrite,
                  Import { from: PathScopes, to: PathScopes }, Export(PathScopes) }
struct CapabilitySet { granted: Vec<Capability> }       // 既定は空 = 拒否
enum ExecutionContext { InitConfigOnly, ShellInput, CodeExplicit, SavedMacro, DslRun, GuiUserAction }
struct SecurityContext { execution: ExecutionContext, granted: CapabilitySet, base_dir: PathBuf }
struct PathScope { root: CanonicalPath, kind: Tree | SingleFile }
struct AuthorizedPath { /* 非公開コンストラクタ。IoGateだけが生成する */ }
trait IoGate { fn authorize(&self, request: &IoRequest) -> Result<AuthorizedPath, IoSecurityError>; }
```

ランタイムのfilesystem関数は `AuthorizedPath` だけを受け取ります。`&Path` を直接受け取る関数を作らないため、gateを通らないI/Oは型の上で書けません。`FileWrite` はcreate/write/append/copy/move/deleteを含み、許可scopeの外へ出られません。`ProjectWrite` は現在のprojectだけが対象で、path scopeを持ちません。

### 実行contextごとの既定権限

| context | 既定 | 付与の方法 |
| --- | --- | --- |
| init.rly / generated.rly / user.rly の自動実行 | `Config` のみ | 付与できません |
| `rowly shell` の直接入力 | 空 | 静的に確定できるpathと必要capabilityだけ、その1 command/1 blockに限り許可します。動的pathや広いscopeは実行前に確認し、session全体へは付与しません |
| Code UIからの明示実行 | 空 | block/macro単位で評価します |
| saved macro | 無効 | preflight表示の後に「今回だけ許可」「この版を許可」「キャンセル」を選びます |
| `rowly dsl run` | 空 | `--allow-read/--allow-write/--allow-project-write/--allow-import-from/--allow-import-to/--allow-export-to <path>` を複数指定できます。不足は `CAPABILITY_DENIED` で即終了します。`--allow-all` は提供しません |
| GUIのImport/Export dialog | 空 | 利用者がdialogで選んだ1つのpathだけを、その操作に限りscopeとして付与する案です(未決事項1) |

### path scope判定の処理

1. relative pathは `SecurityContext::base_dir` で絶対化します。project contextはproject root、bare CSV contextは開いたCSVの親directory、targetなしはprocessのcurrent directoryです(#133)。
2. 存在する最も深い祖先をcanonicalizeし、存在しない残りの成分は `..` を含まない通常成分だけを許可して連結します。
3. scopeとの比較は文字列prefixではなくpathの成分単位で行います。`C:/safe` と `C:/safe-evil` を区別します。Windowsでは大文字小文字を区別せずに比較し、`\\?\` 接頭辞と区切り文字の違いを正規化します。
4. scope root配下の成分にsymlink・junction・reparse pointがある場合は `UnsafePath` として拒否します。
5. 実アクセスの直前に1〜4を再実行し、開いた後はハンドルの最終pathがscope内であることを確認します(Windowsは最終path取得、Unixは `O_NOFOLLOW` 相当)。preflightで得たpathを再利用しません。

採用しなかった案は2つあります。「文字列prefix比較」は `safe-evil` と大小文字差を誤判定するため不採用です。「preflightの結果をruntimeで信用する」はTOCTOUで差し替えられるため不採用です。

### Local File I/O(#133)

命令は `IO.ReadText` / `IO.Exists` / `WRITE IO.Text` / `APPEND IO.Text` / `CREATE IO.Directory` / `COPY IO.File … TO` / `MOVE IO.File … TO` / `DELETE IO.File` です。必要capabilityは次のとおりです。

- `ReadText`・`Exists`: 対象pathが `FileRead` scope内であること。
- `WRITE`・`APPEND`・`CREATE`・`DELETE`: 対象pathが `FileWrite` scope内であること。
- `COPY`: コピー元が `FileRead`、コピー先が `FileWrite` scope内であること。`MOVE`: 移動元・移動先の両方が `FileWrite` scope内であること。

`ReadText` はUTF-8だけを返し、不正なバイト列は `InvalidUtf8` にします。`WRITE` は同じdirectoryに `create_new` で一時ファイルを作り、成功後にrenameで置換します(`csv_io.rs` の方式)。parent directoryは暗黙作成しません。directory deleteは空directoryだけで、再帰削除は1.0にありません。copy/moveの先に既存ファイルがある場合は `DestinationConflict` で止め、種別(file/directory)が違う場合も拒否します。binaryのraw APIは提供しません。

raw `MOVE IO.File` / `DELETE IO.File` はproject manifestとsource_idを更新しません。登録済みsourceの移動・削除はProject I/O Commandを使います。

### IMPORT / EXPORT(#134)

DSLの `IMPORT <FORMAT> <input> TO CSV <output>` と `EXPORT <table> TO <FORMAT> <path>` は、`ImportExport` Commandを呼び、format adapterへ委譲します。DSL runtimeにXLSX処理を持たせません。必要capabilityは、Importが `Import.from`(入力)と `Import.to`(出力CSV)、Exportが `Export` scopeです。

既存の宛先ファイルは、`OVERWRITE` が明示されない限り変更しません。adapterは宛先と同じdirectoryの一時ファイルへ書き、成功後に置換します。現行のbridgeは出力先へ直接 `save` するため、この点を改修します。formatが表現できない値(例: 32767文字を超えるXLSX cell, #218)は、一時ファイルを作る前に位置付きエラーにします。方針は [DATA_MODEL_PRINCIPLES.md](DATA_MODEL_PRINCIPLES.md) の「Lossless-or-error」に従います。Import結果のCSVはproject sourceへ自動登録しません。登録は `ADD This.Project.Source` を別に実行します。Exportは保存ではないため、canonicalのdirty状態を変えません([PERSISTENCE_CONTRACT.md](PERSISTENCE_CONTRACT.md))。

### Project I/O(#130)

`ADD/REMOVE/DELETE/MOVE/RELINK This.Project.Source`、`CREATE/MOVE/DELETE This.Project.Csv|Text|Resource`、`SAVE This.Project` は、#129のProject Command APIだけを呼びます。必要capabilityは `ProjectWrite` です。物理ファイルを変更する操作(`DELETE` `MOVE` `CREATE`)は、さらにその物理pathが `FileWrite` scope内であることを要求する案です(未決事項2)。`SAVE` は `.rwprj` を更新し、`.rowlyx` は #131 の一時archive経由の原子的置換に従います。GUIのproject browserと同じAPIを通し、source_id・manifest・履歴を更新します。

### trust(#135)

saved macroの「この版を許可」は、macro revision fingerprintに結び付けます。fingerprintは、macro本文、実行時に読み込むDSL dependency/include sourceの内容、1.0 grammar/security metadata versionから計算し、1 byteの変更でも再承認を要求します。承認記録は `(fingerprint, 許可したcapabilityとscope)` の組とし、scopeが変わると再承認です。trust storeは端末ローカルに置き、`.rwprj`・`.rowlyx`・generated DSLへ保存しません。保存先の具体は未決事項3です。project全体への常時許可(blanket grant)は提供しません。

## 利用者から見た操作

| 入口 | Command ID案 / 構文 | 失敗時の見え方 |
| --- | --- | --- |
| GUI(View/Edit/Code) | `data.import` `data.export` `data.import.xlsx` `data.export.xlsx`。KeyTipsは `Alt → D → I` / `Alt → D → E` | 実行直前に再検査し、失敗内容をdialogに表示します |
| Command Palette | 同じCommand ID | 同じ |
| DSL | `IMPORT` / `EXPORT` / `IO.*` / `…This.Project…` | `CapabilityDenied`・`PathScopeDenied` などのruntime errorです |
| `rowly dsl run --json` | `--allow-*` 引数 | stdoutはJSONだけです。promptとlogを混ぜず、`error.code` を返します |

GUIはdata menuに、そのbuildで使えるadapterだけを表示します。Code modeは、CSVへ適用済みの状態だけをExport対象にし、未確定・parse error中のtext bufferがある間は `CommandContext` の未適用CSV text判定でExportを無効にします([COMMAND_REGISTRY.md](COMMAND_REGISTRY.md))。canonical dataのExportと、View(Filter/Sort/表示形式)のExport(`view.export.raw_projection`・`view.export.displayed`, #275)は、メニュー上で別の項目にします。drag & drop Importは対象外です。

エラーコード案: `CAPABILITY_DENIED`(CapabilityDenied)、`PATH_SCOPE_DENIED`、`TRUST_REQUIRED`、`UNSAFE_PATH`、`INTERACTION_REQUIRED`。#133のruntime error(FileNotFound・PermissionDenied・DestinationConflict・InvalidUtf8・IsDirectory/NotDirectory・IoError)も同じ形式の安定コードにします。名称は案で、確定は未決事項4です。

## 永続化と互換性

- CSV本文は変更しません。capability・scope・trustの情報をCSVへ書きません。
- `.rwprj`・`.rowlyx`・generated init DSLには、capabilityの許可・trust・path scopeを保存しません。保存するとprojectの受領者が許可を引き継げてしまうためです。
- trust storeは端末ローカルのRowly設定領域に保存します(場所は未決事項3)。projectのコピー・`.rowlyx` の別PC移動では引き継がれません。
- 既存の `INCLUDE GENERATED` / `INCLUDE USER` 構文と、`apply_safe_init` の動作は変えません。#5の資料にはinit内で `IMPORT "./init/viewer.rly"` と書く例がありますが、現行実装は `INCLUDE` です。#134の `IMPORT <FORMAT>` との衝突は未決事項5で扱います。

## 安全性と失敗時の挙動

- 原子性: 書込みは一時ファイル+rename、`.rowlyx` はtemporary archive+置換です。失敗時は元のファイル・packageを残します。
- 順序: 検査(capability→scope→宛先種別→format制約)をすべて通してから一時ファイルを作ります。
- 部分更新: copy/moveの途中失敗では、宛先を作らない、または一時ファイルを削除します。削除できない場合も元ファイルは変更しません。
- 不正入力: `..`、drive切替、混在区切り、大文字小文字差、symlink/junction/reparse point、宛先parentの差し替えは、許可scope外のread/writeになりません。
- `.rowlyx`: ZIP Slip、重複entry、symlink entry、不正な展開、repack失敗の各ケースを拒否します。`extract_to` の展開先を一時directory経由にするか否かは未決事項6です。
- non-interactive: 未許可capabilityでpromptしません。`--allow-all` 相当の迂回optionを作りません。
- 履歴: 一般File I/OとExportはUndo/Redoの保証対象外です。Project I/OはProject Command経由で履歴対象にします。

## 実装順序と依存

| 順 | Issue | 最小単位 | 先行条件 |
| --- | --- | --- | --- |
| 1 | #135 | `CapabilitySet`・`PathScope`・`AuthorizedPath`・`IoGate`、ConfigOnly拒否、runtime拒否経路 | なし(最優先) |
| 2a | #133 | `IO.*` 命令を `IoGate` に接続 | #135 |
| 2b | #137(攻撃fixture) | path/trust/rowlyx用fixtureの追加を実装中から開始 | #135の型が決まり次第、他と並行 |
| 3a | #134 | XLSX adapterを `ImportExport` Commandへ接続、一時ファイル+`OVERWRITE` | #133, #135 |
| 3b | #130 | Project I/O構文をProject Commandへ接続 | #129, #131, #133, #135 |
| 4 | #136 | data menu・KeyTips・dialog | #109, #113, #134 |
| 5 | #137 | 全経路が揃った後に必須gateを完成 | #130, #131, #133, #134, #135 |

3a・3b・(2bの後半)は並行できます。#241・#219・#218 は本文の確認状況に応じて、#134(宛先書込み)と #131(rowlyx pack)に取り込みます。

## 受け入れ条件と検証

Issueの受け入れ条件は次のとおり検証します。

- default deny・init DSLのI/O拒否・未承認macroの拒否(#135, #133, #134, #130): 単体テストと統合テストで、4種のcapability(read/write/project/import-export)をConfigOnlyに対し1つずつ確認します。function/class/間接呼出し/条件分岐/loop/method経由でも拒否されることを含みます。
- path scope(#135, #137): `../`、絶対path切替、`safe`/`safe-evil`、混在区切り、Windowsの大小文字、symlink、Windowsのjunction/reparse point、宛先parentの差し替え。Windowsを正式対象としてjunction、Ubuntuでsymlinkとtraversalを、CIで自動実行します。
- trust(#135, #137): 偽のtrust metadata、`.rowlyx` の別profile移動、本文・dependency・scope変更による失効。
- destructive I/O(#133, #134, #137): 明示OVERWRITEなしの上書き拒否、再帰削除なし、宛先種別不一致の拒否、write/repack失敗時の既存ファイル保持。
- `.rowlyx`: ZIP Slip、重複entry、symlink entry、unsafe extraction、repack失敗。
- non-interactive: 未許可でprompt無し、`--json` のstdoutがJSONのみ、安定した `error.code`、`--allow-all` の不在。
- Import/Export(#134, #136, #218): XLSX export/import、CSV文字列の保持(`001`・`=1+1`)、32767文字超の位置付きエラー(40,000文字fixture)、project外Export、Importの暗黙登録なし。GUIは `Alt → D → I/E` のkeyboard-only起動、Command Palette、Code modeの未確定buffer拒否を実操作で確認します。
- fuzz/property: DSL parserのI/O構文、path正規化、`.rwprj` parser、rowlyx entry検証。
- 1.0 freeze前に #137 の結果を確認し、既知の重大bypassがある状態ではreleaseしません。

## 未決事項

1. GUIのImport/Export dialogの権限。選択: (A) dialogで選んだ1 pathだけをその操作のscopeにする、(B) GUIは権限検査を省略する。推奨はA。理由: #136は「surfaceごとに規則を分岐させない」と定めており、AならGUIも同じ `IoGate` を通せるためです。
2. Project I/Oの物理変更(`DELETE`/`MOVE`/`CREATE`)に `FileWrite` scopeも要求するか。選択: (A) `ProjectWrite` のみ、(B) `ProjectWrite` と `FileWrite`。推奨はB。理由: #135は「`FileWrite` はdeleteを含み、scope外へ出られない」と定めており、projectの外部参照sourceを `ProjectWrite` だけで削除できるとscopeが空洞化するためです。
3. trust storeの保存先と形式。選択: (A) OSの利用者別設定directory内のJSON、(B) OS資格情報ストア。推奨はA。理由: macroのfingerprintと許可scopeの組は秘密情報ではなく、環境構築を利用者に求めずに済むためです。
4. エラーコードの正式名。#135はCLI JSONの `CAPABILITY_DENIED` だけを例示しています。推奨は、他のエラーも大文字SNAKE_CASEで統一する案(本文の案)。理由: 利用者のscriptが文字列で判定しやすいためです。
5. `IMPORT "<path>"`(script include)と `IMPORT <FORMAT> …`(データ変換)の語の衝突。選択: (A) `IMPORT` の直後がFORMAT名なら変換、文字列リテラルならinclude、(B) include側を別語にする。推奨はB(現行の `INCLUDE`)。理由: 同じ語で、実行に必要なcapabilityが全く異なる命令を区別するのは誤読を招くためです。
6. `.rowlyx` の `extract_to` を一時directory+renameにするか。推奨はする。理由: 途中失敗で中途半端な展開先を残さないためです。#219はpack衝突の別Issueであり、本文確認後に統合の要否を決めます。
7. 実装の手段。標準ライブラリと手書きのチェックで進めるか、ハンドル基準のI/O crateを評価するか。推奨は、導入前にWindows junctionの検出可否を試験して決める。理由: 依存追加は配布物とライセンス([LICENSING.md](LICENSING.md))に影響するためです。
