# Rowly 共同開発ポータル

Claude、ChatGPT、Copilot、Codex と人間の開発者は、このファイルを入口にしてください。ここには規則本文を書かず、作業の種類ごとに読む資料への道案内だけを置きます。全ての資料を読む必要はありません。「作業の流れ」と「何をするときに何を読むか」の表から、必要な資料だけを開いてください。

## 絶対に守る5つの原則

1. CSVのraw cell textが正本です。表示・計算規則・derived resultを暗黙にCSVへ書き込みません。
2. GUI・CLI・DSL・スクリプトは、同じapplication/process境界とCommand Registryを通します。
3. 全てのGUI操作は、Alt KeyTipsから到達できるCommandとして登録します。
4. 利用者に環境構築を求めません。実行時依存は配布物へ同梱します。
5. 設計が決まる前に実装しません。設計とIssueが食い違ったら、実装を止めて設計を先に直します。

原則の詳細と、作業開始・設計・終え方・検証の規則本文は [`docs/CONTRIBUTION_RULES.md`](docs/CONTRIBUTION_RULES.md) にあります。

## 作業の流れ

1. **Issueを読む。** 本文・追加決定コメント・関連PRを確認します。
2. **設計書を引く。** [`docs/DESIGN_INDEX.md`](docs/DESIGN_INDEX.md) でIssue番号から設計書を探します。無ければ設計書を書くPRを先に出します。
3. **着手前の確認を通す。** [`docs/READY_CHECKLIST.md`](docs/READY_CHECKLIST.md) の6項目を確認します。
4. **着手記録を残す。** 書式は [`docs/DEVELOPMENT_WORKFLOW.md`](docs/DEVELOPMENT_WORKFLOW.md) にあります。
5. **実装する。** 現在の動作は `src/` とテストで確認します。コード規約は [`コーディング規約.md`](コーディング規約.md) です。
6. **PRを出す。** CI・自己レビュー・merge条件は [`docs/DEVELOPMENT_WORKFLOW.md`](docs/DEVELOPMENT_WORKFLOW.md) の「PR・自己レビュー・merge」に従います。

## 何をするときに何を読むか

| やること | 読む資料 |
| --- | --- |
| 最初に全体像を知る | [`README.md`](README.md)、[`PROJECT_CONTEXT.md`](PROJECT_CONTEXT.md) |
| どの設計書を読むか決める | [`docs/DESIGN_INDEX.md`](docs/DESIGN_INDEX.md) |
| 実装するコードの入口を探す | [`AI_CONTEXT.md`](AI_CONTEXT.md)、[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) |
| コードを書く・レビューする | [`コーディング規約.md`](コーディング規約.md) |
| 文書・コメント・エラー文を書く | [`AI_WRITING_RULES.md`](AI_WRITING_RULES.md) |
| データの扱い（CSV・計算・表示）を変える | [`docs/DATA_MODEL_PRINCIPLES.md`](docs/DATA_MODEL_PRINCIPLES.md) |
| GUI操作を追加する | [`docs/COMMAND_REGISTRY.md`](docs/COMMAND_REGISTRY.md)、[`docs/GUI_FOUNDATIONS.md`](docs/GUI_FOUNDATIONS.md) |
| Rowly DSLを変える | [`docs/DSL_REFERENCE.md`](docs/DSL_REFERENCE.md)、[`docs/DSL_BINDINGS.md`](docs/DSL_BINDINGS.md) |
| 保存・Export・外部同期・状態遷移を変える | [`docs/PERSISTENCE_CONTRACT.md`](docs/PERSISTENCE_CONTRACT.md)、[`docs/MERGE_CONTRACT.md`](docs/MERGE_CONTRACT.md)、[`docs/SESSION_STATE_TRANSITIONS.md`](docs/SESSION_STATE_TRANSITIONS.md) |
| Excel・配布・CIに触れる | [`docs/EXCEL.md`](docs/EXCEL.md)、[`docs/CI.md`](docs/CI.md)、[`docs/DISTRIBUTION.md`](docs/DISTRIBUTION.md) |
| Issueを選ぶ・優先度を付ける・引き継ぐ | [`docs/DEVELOPMENT_WORKFLOW.md`](docs/DEVELOPMENT_WORKFLOW.md)、[`.github/CONTRIBUTING.md`](.github/CONTRIBUTING.md)、[`docs/ISSUE_PRIORITIES.md`](docs/ISSUE_PRIORITIES.md) |
| 設計を変える・新しい設計書を書く | [`docs/READY_CHECKLIST.md`](docs/READY_CHECKLIST.md) |

## 迷ったとき

- 資料を読めない、またはIssueと設計書が食い違うときは、内容を推測せず「未確認」と報告して質問してください。
- 他の担当者のdirty差分を、上書き・削除・一括stageしないでください。
- 通常のClaude ChatやChatGPTを使う場合は、このファイル、対象Issue、base SHA、関連する設計書の節だけを会話へ渡してください。
