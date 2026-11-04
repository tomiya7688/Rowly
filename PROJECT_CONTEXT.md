# Rowlyプロジェクト情報

この文書は、Rowlyで作業するAIへ、変わりにくい事実と詳細資料の場所を伝えます。仕様全文や実装の説明は複製せず、作業に関係する資料を参照してください。

## 目的と利用者

- 目的: CSVを正本に保つ表形式データの閲覧・編集を提供します。
- 主な利用者: CSVを表形式で扱う人です。利用者の職種や業務は限定していません。

## 主な機能と制限

- GUIはTable Editor、Text Editor、Viewerを提供します。GUIの実装入口は `src/gui_main.rs` と `src/gui.rs` です。
- Rowly DSLとLuauはRustのapplication/process境界を通じてCSVを操作します。Python/openpyxlはXLSXとの受け渡しに使います。
- CSVにはraw cell textを保存します。計算規則と表示設定はCSV本文へ書き込みません。derived resultは明示的なMaterialize操作でのみCSV本文へ反映します。
- Calculation Bindingのcell/range対応とproject保存はmainへ統合済みです。Issue #140は、Alt KeyTipsからの到達性などの残件があるためopenです。#140の受け入れ条件を満たしたものとして扱わないでください。Alt KeyTipsは #109 と #113 に依存し、まだ実装されていません。
- Calculation Bindingの直接engine APIはsingle-cell targetです。DSLとGUIはrangeを各cellへ展開します。single-cell結果をCSVへ明示反映するprocess APIは実装済みです。column-relative target、GUIからのMaterialize、View groupingは未実装です。
- bare CSVではproject設定を作らず、Calculation Bindingはsession-onlyです。project内ではGUI生成規則を `generated.rly` に保存し、手書き規則を `user.rly` から読み込みます。
- ProjectSessionが更新したgenerated init/configはsession内へstagingし、`ProjectSession::save()`がmanifestと設定の保存（packageではarchive repackも）に成功した場合だけ確定します。session内のsource再openはstaged設定を反映し、保存失敗時は元fileを復元してstagingを保持します。

## 確認方法

- Rustの確認手順とCI全体は `.github/workflows/ci.yml` を参照してください。
- 主なローカル確認は `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo build --all-targets --all-features` です。
- GUI変更の見た目・操作はビルドだけでは確認できません。必要な画面操作やE2Eが未確認なら、その状態を報告してください。

## 詳細資料

- 製品の原則と利用者向け入口: [`README.md`](README.md)
- 全設計書の索引（Issue番号から設計書を引く）: [`docs/DESIGN_INDEX.md`](docs/DESIGN_INDEX.md)
- 実装前の確認（手戻り防止）: [`docs/READY_CHECKLIST.md`](docs/READY_CHECKLIST.md)
- 構成と依存方向: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- CSV・計算・表示の正本: [`docs/DATA_MODEL_PRINCIPLES.md`](docs/DATA_MODEL_PRINCIPLES.md)
- 対話セッションの未適用編集・未保存状態・変更通知・遷移契約: [`docs/SESSION_STATE_TRANSITIONS.md`](docs/SESSION_STATE_TRANSITIONS.md)
- 外部同期 / Code適用の3-way merge契約: [`docs/MERGE_CONTRACT.md`](docs/MERGE_CONTRACT.md)
- 保存 / 名前を付けて保存 / Pack / Exportの永続化契約: [`docs/PERSISTENCE_CONTRACT.md`](docs/PERSISTENCE_CONTRACT.md)
- GUIコマンド登録 / アプリケーションコマンド契約: [`docs/COMMAND_REGISTRY.md`](docs/COMMAND_REGISTRY.md)
- 非破壊View・Filter・Sort・名前付きView: [`docs/VIEW_MODEL.md`](docs/VIEW_MODEL.md)
- 意味ベースの操作記録・Rowly DSLマクロ生成: [`docs/COMMAND_RECORDING.md`](docs/COMMAND_RECORDING.md)
- 検索・置換: [`docs/SEARCH_REPLACE.md`](docs/SEARCH_REPLACE.md)
- CodeモードCSV編集器: [`docs/CODE_EDITOR.md`](docs/CODE_EDITOR.md)
- 差分・競合インスペクタ: [`docs/DIFF_INSPECTOR.md`](docs/DIFF_INSPECTOR.md)
- データプロファイル・品質確認: [`docs/DATA_PROFILE.md`](docs/DATA_PROFILE.md)
- 表計算機能マップ: [`docs/SPREADSHEET_CAPABILITY_MAP.md`](docs/SPREADSHEET_CAPABILITY_MAP.md)
- Fill / Series / Paste Special: [`docs/EDIT_OPERATIONS.md`](docs/EDIT_OPERATIONS.md)
- 表示書式・条件付き書式: [`docs/DISPLAY_FORMATTING.md`](docs/DISPLAY_FORMATTING.md)
- Calculation標準関数: [`docs/CALCULATION_LIBRARY.md`](docs/CALCULATION_LIBRARY.md)
- Calculation参照モデル: [`docs/CALCULATION_REFERENCES.md`](docs/CALCULATION_REFERENCES.md)
- Group By / Pivot: [`docs/AGGREGATION_PIVOT.md`](docs/AGGREGATION_PIVOT.md)
- Chart: [`docs/CHARTS.md`](docs/CHARTS.md)
- 印刷 / PDF: [`docs/PRINT_OUTPUT.md`](docs/PRINT_OUTPUT.md)
- 明示データ変換: [`docs/DATA_TRANSFORMATIONS.md`](docs/DATA_TRANSFORMATIONS.md)
- workspace navigation: [`docs/WORKSPACE_NAVIGATION.md`](docs/WORKSPACE_NAVIGATION.md)
- Edit入力バー: [`docs/EDIT_INPUT_BAR.md`](docs/EDIT_INPUT_BAR.md)
- New / Save As: [`docs/FILE_LIFECYCLE.md`](docs/FILE_LIFECYCLE.md)
- DSLの構文とBinding: [`docs/DSL_REFERENCE.md`](docs/DSL_REFERENCE.md)、[`docs/DSL_BINDINGS.md`](docs/DSL_BINDINGS.md)
- 現在の動作: `src/` と対応するテスト。文書の説明だけで実装済みとは判断しないでください。

## 仕様・実装を選んだ理由

- CSVを正本にする理由と計算・表示を分離する現在の方針: [`docs/DATA_MODEL_PRINCIPLES.md`](docs/DATA_MODEL_PRINCIPLES.md) の「背景と目的」「基本モデル」「採用判断」。状態: 文書とコードで確認できる現行方針。確認日: 2026-10-09。過去の承認者と決定日は記録されていません。履歴上の理由や承認を推測しないでください。
- 計算機能の実装境界: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) の「Calculation Binding core」。状態: 現行構成の説明。確認日: 2026-10-09。特定の実装選択を採用した過去の理由は別途確認してください。
- 外部同期・Code draft・Save・Openで状態契約を共通化する理由: [`docs/SESSION_STATE_TRANSITIONS.md`](docs/SESSION_STATE_TRANSITIONS.md) の「絶対条件」「変更適用パイプライン」「Transition Guard」「Save Coordinator」。状態: 2026-11-03のGensparkレビューで複数入口の状態同期不一致が確認されたため採用した現行設計。個別実装は #214〜#236 で段階導入する。

今回だけの判断や根拠は、作業依頼または作業メモに残してください。長く有効な理由を設計文書へ記録するときは、理由・根拠・状態・確認日を明記し、内容を利用者が確認してください。提案・未確認・現在の適用要確認を確定済みの仕様として扱わないでください。
