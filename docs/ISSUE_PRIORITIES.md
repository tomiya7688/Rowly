# Issue 優先順位と着手条件

GitHubの共有入口: [開発計画 #204](https://github.com/tomiya7688/Rowly/issues/204)。個別Issue本文の着手案内と優先度・状態ラベルを更新対象にしてください。

確認日: 2026-10-20（日本時間）。対象は確認時点のOpen Issue 52件です。baseは `3ea4952a87e13df169cb3130c30ef31f51c06336` です。この一覧は初回整理の記録であり、以後の優先度・依存・状態の正本は各GitHub Issueです。

P0は進行中のデータ損失・重大な権限迂回などの緊急修正、P1は共通基盤・安全性・リリース判定、P2は依存に沿って進める機能、P3は追加機能・調査・文書です。今回はP0を割り当てていません。優先度は1.0採用・除外の決定を変更しません。

`ready` は最初の実装単位を開始可能、`needs-audit` は既存実装と受け入れ条件の差分確認が先、`blocked` は記載依存の必要契約・検証が揃うまで本体実装待ち、`review` は既存PRを確認、`tracking` は子Issueを集約する親仕様を表します。調査やfixture準備は依存待ちでも進められます。依存先Issue全体を閉じる必要はなく、利用するAPI・設定形式・検証がmainへ統合された証拠を依存元へ記録すれば着手判定を更新できます。

## 最初に選ぶ作業

- GUI担当: #109 のRegistryと共通dispatch。続いて #113 → #200 → #201 を進めます。#109を将来の全機能実装待ちにはしません。
- 安全性担当: #135 のcapability型・context・runtime拒否経路。続いて #133 → #134、#129/#131の確認後に #130 を進めます。
- CLI担当: #121 のparser/help/dispatch。続いて #122、#124 を進めます。#123は #130/#135 の必要契約が揃ってから接続します。
- レビュー担当: #196 の既存PR #197。別PRで同じcoreを作り直しません。
- 監査担当: #60・#63・#129・#131 は実装とテストが存在するため、受け入れ条件との差を先に確認します。#140のローカル差分を引き継ぐ場合は元担当と編集範囲を確認します。

各担当は `docs/DEVELOPMENT_WORKFLOW.md` の着手・引継ぎ手順を使ってください。担当をAIサービス名で固定せず、Issueごとに実装者・レビュー者を記録してください。

## 全Issueの初回整理

| Issue | 優先度 | 状態 | 先行Issue | 次の作業 |
| --- | --- | --- | --- | --- |
| [#4 GUI・DSL・スクリプトを共通コマンド層で統一する](https://github.com/tomiya7688/Rowly/issues/4) | P1 | tracking | なし | 共通境界の親仕様として #109・#129・#130 を追跡する。例示DSLを現行構文と混同しない。 |
| [#5 プロジェクト形式・スクリプト実行・マクロセキュリティ](https://github.com/tomiya7688/Rowly/issues/5) | P1 | tracking | なし | #129・#130・#131・#135・#137 の残件を集約する。 |
| [#8 CSV基本仕様：空値・引用符・改行・制約](https://github.com/tomiya7688/Rowly/issues/8) | P1 | needs-audit | なし | 既存CSVテストと空値・NOT NULLの要件を対応付け、不足条件だけ子タスクへ切り出す。 |
| [#39 外部アプリとの同時編集を前提にCSV変更を検知・3-wayマージする](https://github.com/tomiya7688/Rowly/issues/39) | P1 | tracking | なし | #67・#68 の統合済み範囲と #69 の残件を対応付ける。 |
| [#51 Windowsを公式対応OSとしUbuntuでも動作可能にする](https://github.com/tomiya7688/Rowly/issues/51) | P1 | tracking | なし | #56・#61 と現在のCIを確認し、配布物とGUI実操作の未検証部分を追跡する。 |
| [#55 Rowly DSL 1.0 の全構文適合テストを作成し仕様を凍結する](https://github.com/tomiya7688/Rowly/issues/55) | P1 | blocked | #130, #133, #134, #135, #137, #140 | 入力規則・Calculation・I/O・権限を含む全構文一覧を作り、依存完了後にfreeze判定する。 |
| [#60 Rowlyプロジェクトの generated init DSL にViewer設定を永続化する](https://github.com/tomiya7688/Rowly/issues/60) | P1 | needs-audit | なし | mainのgenerated initと #203 のstagingを再利用し、未対応設定の保存・復元だけを分割する。 |
| [#66 missing sourceを除外してlogical tableをdegraded再生成する](https://github.com/tomiya7688/Rowly/issues/66) | P1 | blocked | #63 | missing sourceのidentityを保ったdegraded再構築とrelinkを実装する。 |
| [#69 ExternalSync後にViewer設定を部分再解決して維持する](https://github.com/tomiya7688/Rowly/issues/69) | P1 | blocked | #60, #63, #65, #66 | 同期後に設定ごとに再解決し、参照切れだけをdormantにする。 |
| [#72 1.0.0前にKadoka UI / ユーザビリティテストを実施する](https://github.com/tomiya7688/Rowly/issues/72) | P1 | blocked | #55, #57, #108, #110, #112, #113, #136, #140, #141, #142, #201 | 機能ごとの実操作チェック表を準備し、候補buildでKadokaの確認を受ける。 |
| [#109 GUI操作をCommand Registryへ登録し全操作をキーボードから実行可能にする](https://github.com/tomiya7688/Rowly/issues/109) | P1 | ready | なし | 既存GUI操作のcommand ID・context・enabled・KeyTips metadataを定義し、Open/Save/Undo/Redoを同じdispatchへ接続する。 |
| [#113 AltキーでExcel風KeyTipsを表示しキーボードだけでUI階層を操作できるようにする](https://github.com/tomiya7688/Rowly/issues/113) | P1 | blocked | #109 | RegistryのmetadataからAlt階層・overlay・Esc/Alt終了・focus遷移を実装する。 |
| [#120 CLIを人間・自動化・AIから扱いやすい安定した機械インターフェースにする](https://github.com/tomiya7688/Rowly/issues/120) | P1 | tracking | なし | CLI契約の親仕様として #118・#121・#122・#123・#124 を追跡する。 |
| [#121 CLI 1.0のcommand treeとglobal optionを実装する](https://github.com/tomiya7688/Rowly/issues/121) | P1 | ready | なし | 既存Excel CLIとCSV省略形を保ち、parser/help/dispatchを独立moduleへ分けてcommand treeを実装する。 |
| [#122 CLI共通JSON envelopeとexit code正規化を実装する](https://github.com/tomiya7688/Rowly/issues/122) | P1 | blocked | #121 | 共通envelopeとerror/exit mappingを導入し、JSON writerを一箇所に集約する。 |
| [#129 Project Command APIでsource / resourceの追加・削除・移動を統一する](https://github.com/tomiya7688/Rowly/issues/129) | P1 | needs-audit | なし | 既存ProjectSessionを監査し、RemoveとDelete、Move時source_id保持、失敗時の復元を操作単位に分ける。 |
| [#131 Project saveを `.rwprj` / `.rowlyx` で原子的に実行する](https://github.com/tomiya7688/Rowly/issues/131) | P1 | needs-audit | なし | #203のstaging/saveを監査し、resource操作後のmanifest・archive失敗時の復元だけを残件化する。 |
| [#135 Rowly DSLのI/O capabilityをdefault deny・path scope付きで強制する](https://github.com/tomiya7688/Rowly/issues/135) | P1 | ready | なし | capability/context/path scopeの共通型とruntime拒否経路を先に実装し、I/O追加時の必須境界を作る。 |
| [#137 1.0リリース前にDSL / Project / File I/Oのセキュリティゲートを通す](https://github.com/tomiya7688/Rowly/issues/137) | P1 | blocked | #123, #130, #131, #133, #134, #135 | 実装中から攻撃fixtureを追加し、全I/O経路が揃った時点で必須security gateを完成させる。 |
| [#140 Calculation BindingをRowly DSL / GUIから割り当ててprojectへ保存する](https://github.com/tomiya7688/Rowly/issues/140) | P1 | blocked | #109, #113 | ローカル作業中のDSL/GUI/project差分を引き継ぎ確認し、不足するRegistry・KeyTips接続を別の実装単位にする。重複着手しない。 |
| [#196 Calculation Bindingのfresh derived resultをprocess APIでMaterializeする](https://github.com/tomiya7688/Rowly/issues/196) | P1 | review | なし | Open PR #197のHEADとCI・レビューを確認する。新しい実装PRを重ねない。 |
| [#200 全GUI CommandがAlt KeyTipsから到達可能であることを自動検証する](https://github.com/tomiya7688/Rowly/issues/200) | P1 | blocked | #109, #113 | 実Registry/context/KeyTips graphを検証するvalidatorと安定した専用test targetを実装する。 |
| [#201 Alt KeyTips全到達性テストをWindows / Ubuntu CIの必須ゲートに追加する](https://github.com/tomiya7688/Rowly/issues/201) | P1 | blocked | #200 | 専用test target確定後、Windows/Ubuntu両方へ独立stepを追加する。 |
| [#1 複数CSVを論理テーブルとして結合表示する](https://github.com/tomiya7688/Rowly/issues/1) | P2 | tracking | なし | #63・#64・#66 の進捗を集約し、物理結合の未分割要件を別タスク化する。 |
| [#2 CSV / JSON / Markdown / XLSX / SQLite の相互変換と保存形式](https://github.com/tomiya7688/Rowly/issues/2) | P2 | tracking | なし | 既存XLSX経路を確認し、JSON・Markdown・SQLiteは形式ごとに入出力契約を分割する。 |
| [#6 View・Edit・Code の3モードを実装する](https://github.com/tomiya7688/Rowly/issues/6) | P2 | tracking | なし | #57・#110・#140・#141 の完了証拠を集約する。 |
| [#7 CSV文字コード：UTF-8標準とShift-JIS互換読み込み](https://github.com/tomiya7688/Rowly/issues/7) | P2 | needs-audit | なし | UTF-8保存の既存実装と、起動ごと1回のShift-JIS確認UIの不足を分けて記録する。 |
| [#9 GUIから列の値で論理シートを分割する](https://github.com/tomiya7688/Rowly/issues/9) | P2 | tracking | なし | #65 のcoreと、GUI設定導線の残件を分離して追跡する。 |
| [#10 コマンド単位の独自 Undo / Redo 履歴をプロジェクト内に保持する](https://github.com/tomiya7688/Rowly/issues/10) | P2 | tracking | なし | #70・#71 と外部競合退避の履歴要件を追跡する。 |
| [#57 eguiでCSV Spreadsheet Gridを実装する](https://github.com/tomiya7688/Rowly/issues/57) | P2 | needs-audit | なし | 既存Gridと選択・構造編集・候補編集の不足を対応付け、#110との編集範囲を分ける。 |
| [#63 複数CSVを同一ヘッダーで論理テーブルへ結合する](https://github.com/tomiya7688/Rowly/issues/63) | P2 | needs-audit | なし | 既存LogicalProject実装と全受け入れ条件を照合し、重複実装せず残件を特定する。 |
| [#64 論理テーブルのdefault書込先とsource間row移動を実装する](https://github.com/tomiya7688/Rowly/issues/64) | P2 | blocked | #63 | 既存default targetとrow moveを監査し、同一schema検証と一括Undoの不足を実装する。 |
| [#65 列値ルールで論理シートを動的分割する](https://github.com/tomiya7688/Rowly/issues/65) | P2 | blocked | #63 | 列値によるsheet所属の再評価を、固定row index保存から分離して実装する。 |
| [#70 プロジェクト内に永続Git風Undo / Redo履歴ストアを実装する](https://github.com/tomiya7688/Rowly/issues/70) | P2 | needs-audit | なし | 既存memory historyとproject save境界を確認し、永続object・HEAD・再openを一つの子タスクに分ける。 |
| [#71 履歴tagと監査可能なrollback commandを実装する](https://github.com/tomiya7688/Rowly/issues/71) | P2 | blocked | #70 | 永続履歴上のtagと、後続履歴を残すrollback commandを実装する。 |
| [#107 DSL入力規則をproject init DSLとして保存・復元する](https://github.com/tomiya7688/Rowly/issues/107) | P2 | blocked | #60, #63, #65, #69 | PR #164のcolumn対応を再実装せず、logical table・cell/range identityとdormantの残件を分割する。 |
| [#108 Edit GUIから入力規則DSLを生成・編集できるようにする](https://github.com/tomiya7688/Rowly/issues/108) | P2 | blocked | #107, #109, #113 | column UIを最初の実装単位とし、AllowedValuesとBoolean式を同じDSL定義へ反映する。 |
| [#110 Spreadsheet選択をCSVの有限データ範囲に基づくキーボード操作として実装する](https://github.com/tomiya7688/Rowly/issues/110) | P2 | blocked | #57 | Grid監査後、有限CSV範囲の選択modelとkeyboard操作を独立単位で実装する。 |
| [#112 上部メニューにキーボードショートカット一覧を追加し Alt→K で開けるようにする](https://github.com/tomiya7688/Rowly/issues/112) | P2 | blocked | #109, #113 | Registryからshortcut一覧を生成し、Alt→Kと一覧内の検索・終了を接続する。 |
| [#118 `rowly shell` で対話型CLI / Rowly DSL REPLを提供する](https://github.com/tomiya7688/Rowly/issues/118) | P2 | blocked | #121, #123, #130 | 継続入力とsession保持を最初の単位にし、meta command・補完・取消を分けて実装する。 |
| [#123 `rowly dsl run` でDSLをfile/stdinからone-shot実行する](https://github.com/tomiya7688/Rowly/issues/123) | P2 | blocked | #121, #122, #130, #135 | file/stdinのDSL実行を既存runtimeに接続する。path-scoped grantを使い、古いBoolean grant例を採用しない。 |
| [#124 `rowly commands` / `rowly capabilities` でCLIを自己記述可能にする](https://github.com/tomiya7688/Rowly/issues/124) | P2 | blocked | #121, #122 | parser metadataからcommandsを生成し、実際のbuild/runtime能力をcapabilitiesに反映する。 |
| [#130 Rowly DSLからProject I/O Commandを操作できるようにする](https://github.com/tomiya7688/Rowly/issues/130) | P2 | blocked | #129, #131, #133, #135 | Project I/O構文をProject Commandへ接続する。Local File I/Oの曖昧な参照は #133 とする。 |
| [#133 Rowly DSLにLocal File I/O命令を追加する](https://github.com/tomiya7688/Rowly/issues/133) | P2 | blocked | #135 | context別path baseとUTF-8 I/Oをruntime capability境界へ接続する。 |
| [#134 Rowly DSLにIMPORT / EXPORT命令を追加し形式adapterを呼び出す](https://github.com/tomiya7688/Rowly/issues/134) | P2 | blocked | #133, #135 | まず実装済みXLSX adapterを共通Import/Export操作へ接続する。未実装形式は対象に加えない。 |
| [#136 GUIの各モードから共通Import / Export Commandを実行できるようにする](https://github.com/tomiya7688/Rowly/issues/136) | P2 | blocked | #109, #113, #134 | データmenuとdialogを共通Import/Exportへ接続する。 |
| [#141 Viewで連続する同値セルを値を失わず視覚結合する](https://github.com/tomiya7688/Rowly/issues/141) | P2 | blocked | #60, #109, #113 | 同列の連続raw同値groupingをView専用modelにし、設定のDSL保存を接続する。 |
| [#142 Calculation Bindingのderived resultを明示的にCSV正本へMaterializeする](https://github.com/tomiya7688/Rowly/issues/142) | P2 | blocked | #109, #113, #140, #196 | #196/PR #197のcoreを再実装せず、GUI/Registry・range原子性・明示unbindの残件を分割する。 |
| [#3 暗号化保存モード：CSV正本を暗号化して保持する](https://github.com/tomiya7688/Rowly/issues/3) | P3 | needs-audit | なし | 鍵管理・復号後の一時データ・保存形式の未決事項を設計タスクとして確定する。暗号実装へ直行しない。 |
| [#44 mrubyによるRubyユーザースクリプト実行基盤を追加する](https://github.com/tomiya7688/Rowly/issues/44) | P3 | needs-audit | なし | mruby選定・組込み方式・権限と資源制限を設計単位に分ける。1.0対象変更は別途決める。 |
| [#81 Rowly Design Principlesを文書化しCC BY 4.0で公開する](https://github.com/tomiya7688/Rowly/issues/81) | P3 | needs-audit | なし | WHY_ROWLYとの役割と文書ライセンスの既存記録を確認し、日本語Design Principlesの不足を埋める。 |
| [#199 Excelで同種操作のkeyboard / GUI / Automation経路がバラバラな例を公式資料から洗い出す](https://github.com/tomiya7688/Rowly/issues/199) | P3 | ready | なし | Microsoft公式資料で具体例とkeyboard到達性・Automation差を照合し、根拠と条件を記録する。 |

## 確認した証拠と未確認範囲

- GitHubのOpen Issue本文・コメントとOpen PRを確認しました。PR #197がOpenであることを確認しましたが、この整理ではPRのテスト成功やmergeを判定していません。
- mainのProjectSession stagingはPR #203の統合履歴で確認しました。ローカルには #140に関係する未コミット差分があります。
- `src/main.rs` の現行CLI、`src/logical_table.rs` の既存モデル、`src/gui/command_bar.rs` のmenu dispatchと、関連するmodule/testの所在を確認しました。全52件の受け入れテストを実行した監査ではありません。
- 現行 `.github/workflows/ci.yml` にKeyTips専用stepはありません。#200/#201の要件を実装済みと扱わないでください。
- #55のfreezeは #130/#133/#134/#135/#137を含む構文・権限検証が完了してから判定します。#72のKadoka実操作確認をAIやヘッドレステストで完了扱いにしないでください。

