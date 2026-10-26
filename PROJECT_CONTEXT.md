# Rowlyプロジェクト情報

この文書は、Rowlyで作業するAIへ、変わりにくい事実と詳細資料の場所を伝えます。仕様全文や実装の説明は複製せず、作業に関係する資料を参照してください。

## 目的と利用者

- 目的: CSVを正本に保つ表形式データの閲覧・編集を提供します。
- 主な利用者: CSVを表形式で扱う人です。利用者の職種や業務は限定していません。

## 主な機能と制限

- GUIはTable Editor、Text Editor、Viewerを提供します。GUIの実装入口は `src/gui_main.rs` と `src/gui.rs` です。
- Rowly DSLとLuauはRustのapplication/process境界を通じてCSVを操作します。Python/openpyxlはXLSXとの受け渡しに使います。
- CSVにはraw cell textを保存します。計算規則、derived result、表示設定はCSV本文へ書き込みません。
- Calculation Bindingのcell/range対応とproject保存はPR #206でmainへ統合されています（2026-10-22確認）。Issue #140は、Alt KeyTipsからの到達性などの残件があるためopenです。#140の受け入れ条件を満たしたものとして扱わないでください。Calculationの登録・削除のAlt KeyTips経路は #109 と #113 に依存し、未実装です。ファイル・履歴・テキスト適用・画面切替・計算欄表示・再計算のAlt経路はPR #209でレビュー中です。
- Calculation Bindingの直接engine APIはsingle-cell targetです。DSLとGUIはrangeを各cellへ展開します。column-relative target、Materialize、View groupingは未実装です。
- bare CSVではproject設定を作らず、Calculation Bindingはsession-onlyです。project内ではGUI生成規則を `generated.rly` に保存し、手書き規則を `user.rly` から読み込みます。
- ProjectSessionが更新したgenerated init/configはsession内へstagingし、`ProjectSession::save()`がmanifestと設定の保存（packageではarchive repackも）に成功した場合だけ確定します。session内のsource再openはstaged設定を反映し、保存失敗時は元fileを復元してstagingを保持します。

## 確認方法

- Rustの確認手順とCI全体は `.github/workflows/ci.yml` を参照してください。
- 主なローカル確認は `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo build --all-targets --all-features` です。
- GUI変更の見た目・操作はビルドだけでは確認できません。必要な画面操作やE2Eが未確認なら、その状態を報告してください。

## 詳細資料

- 製品の原則と利用者向け入口: [`README.md`](README.md)
- 構成と依存方向: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- CSV・計算・表示の正本: [`docs/DATA_MODEL_PRINCIPLES.md`](docs/DATA_MODEL_PRINCIPLES.md)
- DSLの構文とBinding: [`docs/DSL_REFERENCE.md`](docs/DSL_REFERENCE.md)、[`docs/DSL_BINDINGS.md`](docs/DSL_BINDINGS.md)
- 現在の動作: `src/` と対応するテスト。文書の説明だけで実装済みとは判断しないでください。

## 仕様・実装を選んだ理由

- CSVを正本にする理由と計算・表示を分離する現在の方針: [`docs/DATA_MODEL_PRINCIPLES.md`](docs/DATA_MODEL_PRINCIPLES.md) の「背景と目的」「基本モデル」「採用判断」。状態: 文書とコードで確認できる現行方針。確認日: 2026-10-09。過去の承認者と決定日は記録されていません。履歴上の理由や承認を推測しないでください。
- 計算機能の実装境界: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) の「Calculation Binding core」。状態: 現行構成の説明。確認日: 2026-10-09。特定の実装選択を採用した過去の理由は別途確認してください。

今回だけの判断や根拠は、作業依頼または作業メモに残してください。長く有効な理由を設計文書へ記録するときは、理由・根拠・状態・確認日を明記し、内容を利用者が確認してください。提案・未確認・現在の適用要確認を確定済みの仕様として扱わないでください。
