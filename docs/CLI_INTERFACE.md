# CLI 1.0 機械インターフェース設計

人間・shell script・CI・AI agentが同じ公開CLIを安定して使えるように、command tree・JSON envelope・exit code・DSL one-shot・自己記述・REPLの契約を定める設計書です。

- 状態: 提案: Issue本文に基づく設計。利用者未承認
- 確認日: 2026-11-05
- 対象Issue: #120 CLIを人間・自動化・AIから扱いやすい安定した機械インターフェースにする(親) / #121 CLI 1.0のcommand treeとglobal optionを実装する / #122 CLI共通JSON envelopeとexit code正規化を実装する / #123 `rowly dsl run` でDSLをfile/stdinからone-shot実行する / #124 `rowly commands` / `rowly capabilities` でCLIを自己記述可能にする / #118 `rowly shell` で対話型CLI / Rowly DSL REPLを提供する
- 関連: #241(共通アプリケーションコマンド実行器)、#5(プロジェクト形式・マクロセキュリティ)、#136(GUIの各モードからの共通Import / Export Command)、#137(DSL / Project / File I/Oのセキュリティゲート)
- 読む人: CLIのparser・出力・dispatchを実装する人、およびCLIを呼ぶ自動化を書く人

## 背景と目的

現行CLIは、人間向けの文面をstdoutへ出し、失敗を一律の終了コード1で返します。呼び出し側はterminal文面を読み取って成否の理由を推測するしかありません。DSLによる表操作や対話的な連続操作の入口もありません。

本設計は、次を実現します。

- 自動化とAIは「one-shotサブコマンド + `--json`」だけで、文面をscrapeせずに結果と失敗理由を判定できます。
- 人間の連続操作は `rowly shell` が受け持ちます。
- 表データの操作言語はRowly DSLだけにします。CLI専用の第二の表操作言語は作りません。

## 現行実装との差

確認したファイルは `src/main.rs` と `tests/cli_cross_platform.rs` です。

| 項目 | 状態 |
| --- | --- |
| `rowly <csv-path>` による `CliCommand::Inspect` | 実装済み。引数が1個ならCSV pathとして扱います。 |
| `rowly excel import` / `rowly excel export` | 実装済み。`parse_excel_command` が `excel_python::import_workbook` / `export_document` を呼びます。 |
| usageエラーの終了コード2 | 実装済み。`main` が `parse_command` の失敗で返します。 |
| 実行失敗の終了コード | 未正規化。`execute` の失敗は一律 `ExitCode::FAILURE`(1)です。 |
| `--help` / `--version`、global option、JSON出力 | 未実装。`print_usage` が3行のusageをstderrへ出すだけです。 |
| `inspect` / `shell` / `dsl run` / `project` / `validate` / `commands` / `capabilities` | 未実装。 |

コード上の部品は存在します。`rowlyx::pack_project` と `RowlyxArchive::open` / `extract_to`、`project::RowlyProject::load`、`rowly_dsl::parse` / `run`、`project_session` は、command surfaceから呼び出せる公開APIです。DSLの `ExecutionReport` のフィールドは `pub(in crate::rowly_dsl)` で、CLIからは直接読めません。`commands_executed` の取得には公開accessorが要ります。

`tests/cli_cross_platform.rs` は、`missing_python_override_is_reported_without_creating_output` が終了コード `Some(1)` を、`invalid_arguments_return_usage_exit_code_on_both_platforms` が `Some(2)` と `usage:` を検査しています。前者は本設計で終了コード8へ変わります。

## 設計方針

### 契約

- CLIはCSV編集ロジックを持たず、Command / process / project / excel の公開APIへdelegateします。正本のraw cell textは変更しません。
- `--json` 対応commandはstdoutへJSON documentを1個だけ出します。banner・progress・ANSI escapeは混ぜません。
- machine判定は `error.code` とprocess exit codeだけで行います。`message` と人間向け文面は互換性contractに含めません。
- 1.0.xでは、command idとJSON fieldの意味を変えません。field追加は許可し、削除・型変更は移行方針を示すまで行いません。
- 暗黙のsaveはしません。永続化はDSL内の `SAVE` 等に従います。

### command tree

```text
rowly inspect <target>
rowly shell [target]
rowly dsl run <script.rly|-> --target <target> [--sheet <name>] [--allow-... ]
rowly project info <target>   |  project pack <rwprj> -o <rowlyx>  |  project extract <rowlyx> -o <dir>
rowly validate <target>
rowly excel import <xlsx> <csv> [sheet-name]  |  excel export <csv> <xlsx> [sheet-name]
rowly commands  |  rowly capabilities
```

`target` は各commandが対応する範囲で `.csv` / `.rwprj` / `.rowlyx` を受け付けます。`rowly <csv-path>` は `inspect` の互換short-handとして1.0で残し、help・文書では `inspect` を正式syntaxとします。

### global option

`--json` / `--quiet` / `--color <auto|always|never>` / `--no-input` / `--help` / `--version` を共通にします。`--json` は `--color=never` と `--no-input` を含意します。`--quiet` は成功時の説明だけを抑制し、終了コードは変えません。

### JSON envelope

`schema`(固定値 `rowly.cli/v1`)、`ok`、`command`(stable command id)、成功時 `result`、失敗時 `error`(`code`・`message`・`details`)、`warnings` を持ちます。型は `CliEnvelope`、`CliError { code: CliErrorCode, message, details }` を案とし、JSONのserializeは1つのwriter関数に集約します。採用しない案: commandごとに独自に `println!` する方式。理由は、stdoutへの混入とenvelope形式の食い違いを防げないためです。

### exit code正規化

`CliErrorCode` から終了コードへの写像を1箇所に置きます。カテゴリは Issue #120 の固定表(0, 2〜9, 130)に従います。既存errorは `DocumentError` → NOT_FOUND / INVALID_FORMAT、`ExcelError`の実行環境失敗 → ENVIRONMENT_ERROR、`ProjectError` / `RowlyxError` → INVALID_FORMAT / NOT_FOUND、`ParseError` → INVALID_FORMAT(詳細code `DSL_PARSE_ERROR`)、DSL実行失敗 → SCRIPT_FAILED、として分類する案です。分類は詳細error codeで細分化できます。

### 自己記述

command定義のmetadata(id・表示名・引数・option・mutating・対応target種別・JSON対応)を1つの静的定義にし、help・`commands` manifest・parserを同じ定義から生成します。手書きの二重一覧は作りません。`capabilities` は実行時に検出した能力(version、OS、architecture、gui、excel、rwprj、rowlyx、dsl、luau)を返します。`gui` はCargo feature `gui` の有無、`excel` はPython helperの起動可否で決めます。AI専用の非公開commandは作りません。

### shell

`rowly shell` の通常入力はRowly DSLで、`:` prefixのmeta command(`:open` `:close` `:sheet` `:status` `:history` `:help` `:commands` `:clear` `:quit` `:quit!`、任意で `:undo` `:redo`)だけがshell context操作です。継続入力はDSL parserの構文完全性で判定します。transactionとundo/redoはDSLとprocess historyへdelegateし、shell独自の履歴・transaction・データ操作は作りません。入力historyはsession内のみで、diskへ保存しません。

## 利用者から見た操作

- 自動化: `rowly dsl run cleanup.rly --target Project.rwprj --json`、`cat cleanup.rly | rowly dsl run - --target data.csv --json`。
- 人間: `rowly shell Project.rwprj`。dirtyで終了または `:open` する場合は keyboardで Save / Discard / Cancel を選びます。
- GUIとの関係: GUIにCLIの入口は不要です。Alt KeyTipsの対象は既存のGUI操作で、CLIの追加によるGUI入口の変更はありません。
- Command ID案: `inspect`, `shell`, `dsl.run`, `project.info`, `project.pack`, `project.extract`, `validate`, `excel.import`, `excel.export`, `commands`, `capabilities`。`file.open` / `file.save` / `edit.undo` / `edit.redo` は #241 のapplication command IDで、CLI command idとは別の名前空間です。
- 失敗時: human modeはstderrへ説明を出します。JSON modeは `ok: false` のenvelopeをstdoutへ出し、stderrは原則空です。`dsl run` が終了時にdirtyなら、humanはwarning、JSONは `result.dirty = true` で伝えます。

## 永続化と互換性

- CLI自体は設定ファイルも履歴も保存しません。CSV本文・`.rwprj`・`.rowlyx` の形式は変更しません。
- `project pack` / `extract` は `rowlyx::pack_project` / `RowlyxArchive::extract_to` の既存挙動に従います。
- 既存の `excel import|export` の引数形式と標準出力の項目は、`--json` を付けない限り維持します。ただし実行失敗の終了コードは1から正規カテゴリへ変わるため、リリースノートで明記します。
- `rowly excel`(単独引数)は現行テストどおり、`excel` という名前のCSV pathとして `inspect` へ解決します。
- ディレクトリ形式のproject設定ファイル名は #5 で `project.rowly` と書かれ、現行実装は `.rwprj` を使います。CLIは現行実装の `.rwprj` を対象にします。

## 安全性と失敗時の挙動

- `--json` / `--no-input` では、promptを出さず待機もしません。判断が必要なら `INTERACTION_REQUIRED` で終了します。
- DSLを `-` でstdinから読む場合、同じstdinをpromptに使いません。
- `dsl run` のI/O権限は #135 のpath-scoped grantに従い、`--allow-read <path>` `--allow-write <path>` `--allow-project-write` `--allow-import-from <path>` `--allow-import-to <path>` `--allow-export-to <path>` を複数指定できます。`--allow-all` は1.0では提供しません。不足時はpromptせず `CAPABILITY_DENIED` で失敗します。CLI側でcapabilityを迂回しません。
- 書込み系command(`excel import|export`、`project pack|extract`)は、失敗時に出力先を中途半端に残さない既存挙動に依存します。`tests/cli_cross_platform.rs` が出力未作成を確認済みです。
- 処理は #241 の方針に従い、対話なしの `InteractionMode` で共通アプリケーションコマンドへ要求を送ります。確認が必要な結果は `INTERACTION_REQUIRED` に写像します。
- Ctrl+Cは、中断可能なcommandで終了コード130にします。documentとhistoryの不変条件を壊さない時点でだけ中断します。
- JSON protocol documentを生成できない致命的障害だけ、stderrへfallback diagnosticを出します。

## 実装順序と依存

1. #121: parser / help / dispatch / global optionを独立moduleにし、既存の `excel` と `<csv-path>` を維持します。未接続のcommandは成功と表示せず終了コード2ではなく「未対応」を示す失敗にします(扱いは未決事項1)。先行条件なし。
2. #122: envelope・`CliErrorCode`・exit code写像・JSON writerを導入します。先行は #121。
3. #124: command定義metadataからhelpと `commands` / `capabilities` を生成します。先行は #121・#122。2と並行して定義metadataの設計だけ先行できます。
4. #123: DSL file/stdin実行を接続します。先行は #121・#122・#130・#135。権限不要のDSLだけを対象にする最小版は #130/#135 を待たずに作れます。
5. #118: shell。先行は #121・#123・#130。継続入力とsession保持、次にmeta command・補完・取消の順に分けます。
6. #241 のRequest/Outcome型は、2のexit code写像と並行して設計できます。CLI側は `Outcome` のみに依存します。

## 受け入れ条件と検証

| 対応Issue | 検証 |
| --- | --- |
| #121 | parserの単体テストでcommand tree・`--help` / `--version`・不正引数の終了コード2を確認します。`tests/cli_cross_platform.rs` の既存Excel・CSV省略形を回帰として維持します。 |
| #122 | 統合テストでstdoutがJSON 1個だけであること、ANSI/promptの混入なし、同じカテゴリが全commandで同じ終了コードになることを確認します。 |
| #124 | `commands --json` がhelpの全commandを列挙すること、Python helper不在時に `excel: false` となることを確認します。 |
| #123 | SAVEなしの非保存、stdinとpromptの非共用、権限不足の即時失敗を統合テストで確認します。 |
| #118 | 継続入力・Ctrl+C・dirty終了・履歴の非永続化を確認します。端末操作は実操作で確認します。 |

CIは `.github/workflows/ci.yml` のWindows/Ubuntu双方で実行します。静的検査・単体テスト・実操作・配布物検証は区別して報告してください。

## 未決事項

1. 未接続commandの終了コード。選択肢: (A) 終了コード2(CLI_USAGE)、(B) 終了コード9(INTERNAL_ERROR)、(C) 未接続commandをparserに載せない。推奨はC。理由は、#121の完了確認が「未接続機能は成功と表示しない」で、manifestに載せないことで存在しない機能を公開しないためです。
2. `INTERACTION_REQUIRED` と `CAPABILITY_DENIED` の終了コード。#120の固定表にありません。選択肢: (A) 6(CONFLICT)に含める、(B) 5(VALIDATION_FAILED)に含める、(C) 新カテゴリを足す。推奨はC(新しい固定コードは1.0の表の改訂を伴うため承認が必要)。決まるまでは暫定的にA/Bを使いません。
3. command idの命名。#120/#122は `inspect`・`validate` を単語、`project.info` をdot区切りで例示します。推奨は、単語だけのcommandも維持して例示どおりに固定することです。理由は、Issueの例と矛盾しないためです。
4. `commands` / `capabilities` の `result` のJSON field名の詳細(`targets` の値の表記など)は #124 の例が最小限です。推奨は、例のfieldを固定し他は追加fieldとして実装時にレビューすることです。
