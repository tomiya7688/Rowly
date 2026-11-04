# 設計書索引

この文書は、Issue番号から設計書を引くための索引です。実装者は [`READY_CHECKLIST.md`](READY_CHECKLIST.md) の確認で、対象Issueに対応する行の設計書を読んでください。入口は [`AGENTS.md`](../AGENTS.md) です。

## 使い方

1. 対象Issueの番号を、下の表の「対象Issue」から探します。
2. 該当行の「設計書」を開き、冒頭の状態（提案 / 確定 / 実装中 / 実装済み）と「未決事項」を読みます。
3. 状態が「提案」で未決事項が実装に影響する場合は、実装せず利用者に判断を求めてください。
4. 該当行が無いIssueは、設計書が無い状態です。設計書を書くPRを先に出してください（書き方は [`READY_CHECKLIST.md`](READY_CHECKLIST.md)）。

状態の列は、各設計書の冒頭に書かれた内容です。古くなっている場合は、設計書の冒頭を正本にしてください。

## 土台（データ・状態・保存・Command）

| 領域 | 設計書 | 対象Issue |
| --- | --- | --- |
| CSV・計算・表示の分離原則 | [`DATA_MODEL_PRINCIPLES.md`](DATA_MODEL_PRINCIPLES.md)、[`ARCHITECTURE.md`](ARCHITECTURE.md) | 全Issueの前提。Calculation Binding core（#139）、Materialize（#142、#196） |
| セッション状態・変更通知・遷移 | [`SESSION_STATE_TRANSITIONS.md`](SESSION_STATE_TRANSITIONS.md) | #214〜#217 #223 #234〜#238 #242 #244 |
| 外部同期 / Code適用の3-way merge | [`MERGE_CONTRACT.md`](MERGE_CONTRACT.md) | #239 #214 #215 #242 |
| 保存 / 名前を付けて保存 / Pack / Export | [`PERSISTENCE_CONTRACT.md`](PERSISTENCE_CONTRACT.md) | #240 #236 #219 |
| アプリケーションコマンド / GUI Command Registry | [`COMMAND_REGISTRY.md`](COMMAND_REGISTRY.md) | #241 #109 #113 |
| Undo / Redoの永続履歴 | [`UNDO_HISTORY.md`](UNDO_HISTORY.md) | #10 #70 #71 #243 |
| CSV仕様・文字コード・形式変換 | [`CSV_AND_FORMATS.md`](CSV_AND_FORMATS.md)、[`EXCEL.md`](EXCEL.md) | #2 #3 #7 #8 #44 #207 #218 |
| Projectモデル・設定の永続化 | [`PROJECT_API_AND_INIT_DSL.md`](PROJECT_API_AND_INIT_DSL.md) | #129 #131 #60 #69 #107 #108 |
| 論理テーブル | [`LOGICAL_TABLES.md`](LOGICAL_TABLES.md) | #1 #9 #63 #64 #65 #66 #141 |

## 言語・スクリプト・CLI

| 領域 | 設計書 | 対象Issue |
| --- | --- | --- |
| Rowly DSL（構文・Binding・標準関数） | [`DSL_REFERENCE.md`](DSL_REFERENCE.md)、[`DSL_BINDINGS.md`](DSL_BINDINGS.md)、[`DSL_STANDARD_LIBRARY.md`](DSL_STANDARD_LIBRARY.md) | #55 #140 |
| DSL / Project / File I/Oのセキュリティ | [`DSL_IO_SECURITY.md`](DSL_IO_SECURITY.md) | #5 #130 #133 #134 #135 #136 #137 |
| Luau | [`LUAU.md`](LUAU.md)、[`LUAU_SANDBOX.md`](LUAU_SANDBOX.md) | #5 |
| CLI 1.0 | [`CLI_INTERFACE.md`](CLI_INTERFACE.md) | #118 #120 #121 #122 #123 #124 |
| 操作の記録とDSLマクロ | [`COMMAND_RECORDING.md`](COMMAND_RECORDING.md) | #249〜#252 #254 #255 |

## GUIと表計算機能

| 領域 | 設計書 | 対象Issue |
| --- | --- | --- |
| GUIの土台（3モード・Grid・KeyTips・フォント） | [`GUI_FOUNDATIONS.md`](GUI_FOUNDATIONS.md) | #6 #57 #109 #110 #112 #113 #200 #201 #230 |
| 表計算に必要な機能の全体地図 | [`SPREADSHEET_CAPABILITY_MAP.md`](SPREADSHEET_CAPABILITY_MAP.md) | #277 |
| View・Filter・Sort・名前付きView | [`VIEW_MODEL.md`](VIEW_MODEL.md) | #245〜#248 #253 #262〜#264 |
| 検索・置換 | [`SEARCH_REPLACE.md`](SEARCH_REPLACE.md) | #256〜#259 |
| Codeモード | [`CODE_EDITOR.md`](CODE_EDITOR.md) | #260 #261 #265〜#267 |
| 差分・競合インスペクタ | [`DIFF_INSPECTOR.md`](DIFF_INSPECTOR.md) | #268〜#271 |
| データプロファイル | [`DATA_PROFILE.md`](DATA_PROFILE.md) | #272〜#274 |
| Fill・Series・Paste Special | [`EDIT_OPERATIONS.md`](EDIT_OPERATIONS.md) | #278〜#282 |
| 表示書式・条件付き書式 | [`DISPLAY_FORMATTING.md`](DISPLAY_FORMATTING.md) | #283〜#288 |
| Calculation標準関数 | [`CALCULATION_LIBRARY.md`](CALCULATION_LIBRARY.md) | #289〜#295 #328〜#330 |
| Calculation参照モデル | [`CALCULATION_REFERENCES.md`](CALCULATION_REFERENCES.md) | #325〜#327 |
| Group By・Pivot | [`AGGREGATION_PIVOT.md`](AGGREGATION_PIVOT.md) | #296〜#300 |
| Chart | [`CHARTS.md`](CHARTS.md) | #301〜#305 |
| 印刷・PDF | [`PRINT_OUTPUT.md`](PRINT_OUTPUT.md) | #306〜#310 |
| 明示データ変換 | [`DATA_TRANSFORMATIONS.md`](DATA_TRANSFORMATIONS.md) | #311〜#316 |
| workspace tab・ナビゲーション | [`WORKSPACE_NAVIGATION.md`](WORKSPACE_NAVIGATION.md) | #317〜#320 |
| Edit入力バー | [`EDIT_INPUT_BAR.md`](EDIT_INPUT_BAR.md) | #321〜#324 |
| New / Open / Save As | [`FILE_LIFECYCLE.md`](FILE_LIFECYCLE.md) | #331〜#335 |

## ビルド・配布・開発運用

| 領域 | 設計書 | 対象Issue |
| --- | --- | --- |
| ビルド・配布・サプライチェーン | [`RELEASE_ENGINEERING.md`](RELEASE_ENGINEERING.md)、[`CI.md`](CI.md)、[`DISTRIBUTION.md`](DISTRIBUTION.md)、[`LICENSING.md`](LICENSING.md) | #51 #72 #81 #220 #221 #222 #224 #225 #227 |
| Issue運用・優先度・共同開発 | [`DEVELOPMENT_WORKFLOW.md`](DEVELOPMENT_WORKFLOW.md)、[`ISSUE_PRIORITIES.md`](ISSUE_PRIORITIES.md)、[`READY_CHECKLIST.md`](READY_CHECKLIST.md) | #204 #210 #211 |
| 製品の動機 | [`WHY_ROWLY.md`](WHY_ROWLY.md) | #81 |

## 設計書が無い、または対象が曖昧なIssue

次のIssueは、専用の設計書がありません。着手前に [`READY_CHECKLIST.md`](READY_CHECKLIST.md) に従って、設計の扱いを決めてください。

- #199 Excelで同種操作の経路がバラバラな例の調査: 調査Issue。成果は [`COMMAND_REGISTRY.md`](COMMAND_REGISTRY.md) の根拠に使います。
- #275 現在Viewの正本Exportと区別したExport、#276 raw値コピーと表示値コピーの分離: [`SPREADSHEET_CAPABILITY_MAP.md`](SPREADSHEET_CAPABILITY_MAP.md) の「View Export / Copy」に項目だけがあり、専用の設計書がありません。[`VIEW_MODEL.md`](VIEW_MODEL.md) と [`PERSISTENCE_CONTRACT.md`](PERSISTENCE_CONTRACT.md) を読んだうえで、設計書を追加してから実装してください。`n- #207 不定幅CSVの保存エラー: [`CSV_AND_FORMATS.md`](CSV_AND_FORMATS.md) の「現行実装との差」に整理済みです。バグ修正として扱えます。

この索引にない新しいIssueを作るときは、設計書の節を決めたうえで、この表に1行を追加してください。
