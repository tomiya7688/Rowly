# 開発規則（AGENTS.mdが案内する規則本文）

この文書は、Rowlyで実装・レビュー・文書作成をする人とAIが守る規則の本文です。入口は [`AGENTS.md`](../AGENTS.md) で、作業の種類ごとに読む資料は [`AGENTS.md`](../AGENTS.md) の表から選んでください。手順は [`DEVELOPMENT_WORKFLOW.md`](DEVELOPMENT_WORKFLOW.md)、設計書の所在は [`DESIGN_INDEX.md`](DESIGN_INDEX.md) にあります。

## 変更を始める条件

- 作業開始のたびに、Git root、branch、HEAD、dirty差分を確認し、`git fetch` で remote main と関連PRを更新してください。作業用branchがremoteより遅れている場合は `git pull --ff-only` で取り込んでください。他者のdirty差分を失う操作は行わず、fast-forwardできない場合は原因を報告してください。取得できなければ、remote鮮度は未確認と報告してください。
- Issueに目的・対象範囲・対象外・依存・受け入れ条件・検証方法が揃い、対応する設計書の節が決まってから実装してください。不足する項目の調査は実装前に進めてください。設計書が無い、または設計書とIssueが食い違う場合は、実装せず [`READY_CHECKLIST.md`](READY_CHECKLIST.md) の手順で設計を先に直してください。
- 着手記録に担当する人と使用ツール、branch、base SHA、編集範囲、次の作業を残してください。AIサービス名をGitHubユーザーのassigneeとして登録しないでください。
- 他の担当者と編集ファイルが重なる場合は先に分担を調整してください。同一checkoutでbranchを切り替えたり、他者のdirty差分を上書き・削除・一括stageしたりしないでください。
- 一つのPRは独立して確認できる一つの変更に絞ってください。親Issue全体や無関係なrefactorを同時に実装しないでください。

## 設計と安全性

- CSVのraw cell textを正本に保ち、表示・計算規則・derived resultを暗黙にCSVへ書き込まないでください。
- GUI・CLI・DSL・スクリプトは既存のapplication/process境界を通してください。CSV codecや独自の履歴モデルへ迂回しないでください。
- 全てのGUI操作は、Alt KeyTipsから到達できるCommandとして登録してください。契約は [`COMMAND_REGISTRY.md`](COMMAND_REGISTRY.md) にあります。
- 利用者に環境構築を求めないでください。Python、openpyxl、その他の実行時依存は、PyInstallerなどで配布物へ同梱し、[`EXCEL.md`](EXCEL.md) と [`CI.md`](CI.md) の配布方針に従ってください。ソースcheckoutの開発用fallbackで必要な準備は、開発者向け文書にだけ書き、利用者向けの手順やエラー文へ出さないでください。
- 仕様・保存形式・依存境界を変える場合は、[`PROJECT_CONTEXT.md`](../PROJECT_CONTEXT.md) が案内する関連文書の節を確認してください。現行仕様、採用理由、根拠、状態、確認日を分けて記録してください。
- 過去の理由や承認をコードから推測しないでください。未記録の理由が今回の変更を左右する場合は未確認と報告して質問し、理由に依存しない調査を続けてください。
- 今回限りの判断はIssueまたは作業用Context Packへ残してください。長期のProject Contextへ複製しないでください。
- 対象範囲内で文章の曖昧さや事実との不一致を発見した場合は、根拠を確認して同じ作業で修正してください。

## 作業の終え方

- 作業が一区切りしたら、独立して確認できる単位でPRを作成してください。PR作成の手順、CIの確認、自己レビュー、merge条件は [`DEVELOPMENT_WORKFLOW.md`](DEVELOPMENT_WORKFLOW.md) の「PR・自己レビュー・merge」に従ってください。
- 必須CIがすべて成功し、自己レビューで未解決の懸念がない場合に限り、PRをmergeしてください。条件を満たせない場合はmergeせず、理由と次の作業を報告してください。

## 検証と引継ぎ

- 機能変更は関連する成功・失敗・境界条件を検証し、`.github/workflows/ci.yml` の該当チェックを実行してください。
- 静的検査、単体テスト、GUI実操作、Windows/Ubuntu CI、配布物検証、merge済みを区別して報告してください。未実行の検証を成功と記載しないでください。
- 必須CI失敗、安全性・正しさの未解決懸念がある変更を承認しないでください。AIの自己評価や別AIの賛同だけで合格にしないでください。
- 中断・担当交代時は、HEAD、未コミット差分、実行した検証と結果、未確認事項、次の一手をIssueへ残してください。ローカルファイルしか読めないAIは、投稿用の引継ぎ本文を成果物として返してください。
- PRの一部実装やローカルテスト成功だけで親Issueを閉じないでください。全受け入れ条件と統合先の証拠を確認してから完了としてください。
