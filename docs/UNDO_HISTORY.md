# コマンド単位の永続Undo / Redo履歴

Rowly独自のコマンド単位Undo / Redo履歴をProject内へ永続保存し、tag・rollback・複数ソース原子的Undoまでを一貫して扱う設計です。

- 状態: 提案: Issue本文に基づく設計。利用者未承認
- 確認日: 2026-11-05
- 対象Issue: #10 コマンド単位の独自 Undo / Redo 履歴をプロジェクト内に保持する（親）、#70 プロジェクト内に永続Git風Undo / Redo履歴ストアを実装する、#71 履歴tagと監査可能なrollback commandを実装する、#243 複数ソース操作を操作ID単位で原子的にUndo / Redoできる履歴グループを実装する
- 関連Issue: #235 CsvDocumentに文書世代と変更通知を導入し全ての正本変更を報告する、#241 共通アプリケーションコマンド実行器、#261 CodeDraft専用Undo / Redo
- 読む人: 履歴ストア、Undo / Redo、保存処理、`.rowlyx` packを実装する人

## 背景と目的

現在のUndo / Redoは `CsvDocument` のメモリ上にだけあり、Projectを閉じると失われます。別PCへ移したProjectや `.rowlyx` で受け渡したProjectでは、編集の取り消しや過去状態への復帰ができません。複数のCSVを一つの操作で変更する場合に、sourceごとに別のUndoへ分裂する問題もあります。

目的は次の四点です。

1. Undo / Redoの基本単位を、セルではなくCommand Layer上のコマンド / トランザクションにします。
2. 履歴をAppData等の端末ローカル領域へ置かず、Projectと一緒に持ち運べる形で保存します。Gitそのものは使いません（Issue #10 が採用しないと決めています）。
3. 名前付きtagと、履歴を消さずに過去状態へ戻すrollbackを監査可能にします。
4. 複数ソースの操作を、操作ID単位で全体が成功するか全体が失敗するかのどちらかにします。

CSVのraw cell textが唯一の正本である原則は変えません。履歴は補助情報です。履歴が無い、または破損していても、CSVは通常のCSVとして開けます。

## 現行実装との差

確認したファイルは `src/process/history.rs`、`src/process/document.rs`、`src/rowlyx.rs` です。

| 項目 | 現状 |
| --- | --- |
| 実装済み | `EditHistory`（`undo` / `redo` のcommand stack、保存済みCSV状態との比較によるdirty判定、`begin_transaction` / `commit_transaction` による複数操作のBatch化） |
| 実装済み | `EditOperation`（`Cells` / `Rows` / `Columns` / `Contents` / `ValidationRules` / `Batch`）がbefore / afterの値を保持し、逆操作を可能にしている |
| 実装済み | `CsvDocument::undo` / `redo` が `apply_command` で適用し、失敗時は `restore_undo` / `restore_redo` でstackを戻す |
| 実装済み | `CsvDocument::rollback_transaction` が、編集中のtransactionを逆順に適用して取り消す |
| 制限あり | `MAX_UNDO_COMMANDS = 256` を超えた古いcommandは捨てる。`record_committed` は新規編集時に `redo` を `clear` するため、旧redo系列は残らない |
| 未実装 | ディスクへの永続化、content-addressed object、HEAD参照、tag、監査可能なrollback command |
| 未実装 | `OperationId`、`SessionChangeSet`、複数 `CsvDocument` を束ねるundo group（`docs/SESSION_STATE_TRANSITIONS.md` の設計のみ） |
| 未実装 | 履歴保持上限の設定、ディスク使用量の管理、GC |
| 未確認 | `.rowlyx` のpackは `collect_directory` でproject root配下のdirectoryを再帰収集する。`data/history/` が実際にpack対象になるかは #70 の実装時にtestで確認する |

`ValidationRules` はsession限定の規則であり、CSV保存では永続化しません。この履歴への記録方法は未決事項に挙げます。

## 設計方針

### 契約

1. Undo / Redoの単位は、Application Commandまたはトランザクション一つです。内部のセル差分の数は実装詳細です。
2. 永続履歴のobjectは作成後に変更しません（immutable）。
3. 履歴は新しいnodeを追加するだけで、既存nodeを書き換えません。Undo後に別の編集をしても、旧redo系列は別の枝として残します。
4. rollbackは過去状態へ戻す新しいnodeを追加します。後続履歴を削除しません。
5. 履歴をすべて削除しても、CSVとProject設定の意味は変わりません。
6. GUIとRowly DSLの実行はUndo / Redoを保証します。Python / LuauはRowlyとして保証しません（Issue #10）。

### データモデル案

保存場所はIssue #70 に従い `data/history/` とします。

```text
data/history/
├─ HEAD            現在のnode idを指す
├─ refs/
│  └─ tags/<name>  tagが指すnode id
└─ objects/
   ├─ command/     HistoryNode(kind = Command)
   ├─ transaction/ HistoryNode(kind = Transaction / Rollback)
   └─ blob/        削除行など逆操作用の内容
```

| 型名（案） | 主なフィールド | 役割 |
| --- | --- | --- |
| `HistoryNodeId` | content hashを表す文字列 | node本体のcontent-addressed id。永続IDはこれだけです |
| `HistoryNode` | `parents`、`kind`、`entries`、`created_at`、`schema_version` | 一つの操作を表すimmutable object |
| `HistoryEntry` | `source`、`EditOperation`相当のpayload、blob参照 | sourceごとの逆操作可能な変更 |
| `HistoryRef` | 名前、対象`HistoryNodeId` | HEADとtagの参照 |

`HistoryNode.kind` は `Command`、`Transaction`、`Rollback { target }` を持ちます。`schema_version` により、将来checkpointやsnapshotのnodeを追加できるようにします。

### 処理の流れ

1. Application Commandの実行が成功すると、変更を束ねた `HistoryNode` を作ります。
2. 逆操作に必要な内容を `blob/` へ、nodeを `objects/` へ書き、最後に `HEAD` を更新します（書込み順序は「安全性と失敗時の挙動」）。
3. Undoは `HEAD` を親へ動かし、Redoは直前に離れた子へ動かします。別の子を作る編集は、親から枝分かれします。
4. 起動時の再openでは `HEAD` からmemory上のundo / redo stackを再構築します。保持上限を超える古い部分は読み込みません。

### 採用しなかった案

| 案 | 採用しない理由 |
| --- | --- |
| 内部Gitによるスナップショット | AppData等へ履歴が置かれ、Projectと一緒に輸送できません（Issue #10）。 |
| 端末ローカルへ履歴を保存する | 別環境でUndo / Redoを継続できません。 |
| `OperationId` を永続IDにする | `OperationId` はsession-localです。永続履歴は別のimmutable node identityを持ちます（`docs/SESSION_STATE_TRANSITIONS.md` の「操作IDと変更グループ」）。 |
| セル単位でUndoする | 貼り付けやDSL実行が複数Undoに分裂します。 |

### 複数ソース操作の履歴グループ（#243）

Application Commandごとに `OperationId` を発行し、`SessionChangeSet` が複数の `DocumentChangeSet` を束ねます。仕様の正本は `docs/SESSION_STATE_TRANSITIONS.md` の「アプリケーション層のUndoグループ」です。本書では永続化との接続だけを定めます。

- 複数sourceを変更する操作は、一つの `HistoryNode` に複数の `HistoryEntry` を持たせます。sourceごとにnodeを分けません。
- Undo / Redoは、全sourceへの適用を先に検証し、すべて成功した場合だけ確定します。途中で失敗した場合は、どのsourceも変更されなかった状態へ戻します。
- 各 `CsvDocument` の内部historyは物理sourceの編集証拠として残し、source間の調停はアプリケーション層のundo groupが担当します。
- 最初の接続対象は #64 のsource間row moveです。`EDIT_OPERATIONS.md` の「Undo」節にある「OperationIdで1 Undo groupへ束ねる」という要件を満たします。

### 履歴保持上限

上限は二種類に分けます。

- メモリ上限: memory上のundo / redo stackの長さ。現行の256を既定値として維持し、設定可能にするかは未決事項にします。
- ディスク上限: `data/history/` の合計サイズまたはnode数。GCは #70 の非対象なので、本書ではGC未実装時に上限を超えた場合の挙動だけを決めます。上限超過時は新しい編集を止めず、警告を表示して履歴を追加し続けます。利用者が履歴を整理する操作は、将来のGC Issueで扱います。

tagが指すnodeと、そのnodeへ到達するために必要な祖先は、上限による破棄の対象から外します。tagが付いた状態へのrollbackが後から失敗することを防ぐためです。

### 別履歴との関係

| 履歴 | 保存先 | 本書との関係 |
| --- | --- | --- |
| canonical CSV履歴 | 永続履歴（本書） | 本書の対象です。 |
| Code draftのtext履歴 | メモリ上 | 本書の対象外です。Code Undoはcanonical履歴を変えません。Apply成功時に、Code変更全体がcanonical履歴の一つの `HistoryNode` になります（`docs/CODE_EDITOR.md` の「2種類の履歴」、Issue #261）。 |
| 外部同期・競合退避 | `ConflictDraft` | 外部ディスク側の変更自体をLocalのUndo commandにはしません。同期後に再適用したLocal変更はUndo可能です（`docs/MERGE_CONTRACT.md`）。 |

## 利用者から見た操作

Command ID案は次のとおりです。既存の `edit.undo` / `edit.redo` は `docs/COMMAND_REGISTRY.md` の定義を変えず、内部の履歴だけが永続化されます。

| 操作 | Command ID案 | GUI入口 | CLI / DSL |
| --- | --- | --- | --- |
| Undo / Redo | `edit.undo` / `edit.redo` | 既存（Alt: E → U / R） | #241 のApplication Command経由 |
| tag作成 | `history.tag.create` | Alt KeyTips配下に割り当てる | `history tag create <name>` |
| tag名変更 / 削除 | `history.tag.rename` / `history.tag.delete` | 同上 | `history tag rename` / `delete` |
| rollback | `history.rollback` | 同上 | `history rollback <tag>` |

tag browserとdiff viewerは #71 の非対象です。GUI入口を追加する場合も、全操作をAlt KeyTipsから到達できるようにしてください。KeyTips文字の割当ては `docs/COMMAND_REGISTRY.md` の登録規則に従います。

失敗時の見え方は次のとおりです。

- 存在しないtag名へのrollbackは、tag名を示すエラーを返し、状態を変えません。
- 同名tagの作成は拒否し、既存tagが指すnodeを示します。
- 履歴の保存に失敗した場合は、編集を確定せず、失敗したパスと原因を表示します。
- 対話なしのCLIでrollbackに確認が必要な場合は、確認不足を示す結果を返します（#241 の契約）。

## 永続化と互換性

- CSV本文は一切変更しません。tag名やnode idをCSVへ書きません。
- 履歴は `data/history/` に保存し、`.rwprj` には履歴の位置とschema versionだけを記録します（記録項目の最終形は未決事項）。
- `.rwprj → .rowlyx → .rwprj` の往復で履歴を失わないことが受け入れ条件です。`pack_project` が `data/history/` を含めること、symlinkを拒否する既存検査（`RowlyxError::UnsupportedLink`）を履歴に対しても維持することを、testで確認します。
- 履歴が存在しないProject、または `schema_version` が未知の履歴は、履歴なしとして開きます。CSVの読込みは妨げません。履歴を使えない理由は利用者へ表示します。
- 古いRowlyで開いて保存した場合、履歴は読み取られず、変更された状態とHEADが食い違い得ます。再open時にHEADが指す状態とCSVの内容が一致しなければ、履歴を現在のCSVへ接続できないとして、履歴を新しい起点から始めます。旧履歴は削除せず残します。

## 安全性と失敗時の挙動

- 書込み順序は、blob、node、refs（tag）、HEADの順です。HEADの更新は一時ファイル作成後のrenameで行い、中途半端なHEADを残しにくくします。
- 孤立したobjectは、HEADから到達できなくても削除しません（GCは対象外）。
- 読込み時にobject idとcontent hashが一致しない場合は、そのnodeを破損として扱い、そこから先の履歴を使いません。CSV本体は開けます。
- Undo / Redo / rollbackは、適用前に全sourceの現在内容が履歴の想定と一致することを検査します。一致しなければ何も変更せず失敗を返します。
- rollbackは「現在HEADの子として新しい `Rollback` node」を追加します。rollback自体がUndo可能です。
- Python / Luauの変更は履歴の保証外です。Command APIを経由した変更が履歴へ載る範囲は将来検討とします。
- 利用者の秘密情報を `data/history/` に含めないため、blobに保存する内容はCSV本文の範囲に限ります。

## 実装順序と依存

| 順序 | Issue | 最小単位 | 先行条件 |
| --- | --- | --- | --- |
| 1 | #235 | `DocumentRevision` と `DocumentChangeSet` | なし。#243 の入力になります |
| 1（並行可） | #70 | object、parent、HEAD、atomic write、再open | 既存 `EditOperation` の直列化形式の決定のみ |
| 2 | #243 | `OperationId` とundo group（メモリ上） | #235。#70 とは独立に実装できます |
| 3 | #70 続き | undo groupからの `HistoryNode` 生成 | #70 本体と #243 |
| 3（並行可） | #261 | CodeDraft専用履歴 | #215。永続履歴とは独立 |
| 4 | #71 | tag、rollback | #70 |
| 4（並行可） | #241 | `edit.undo` / `edit.redo` の共通実行器 | #235 |

#70 とメモリ上の #243 は並行できます。永続化で `OperationId` を `HistoryNodeId` にしないため、依存は「生成元」の一方向だけです。

## 受け入れ条件と検証

| Issue | 受け入れ条件 | 検証の種類 |
| --- | --- | --- |
| #70 | close / open後もUndo / Redoを継続できる | 統合test（`CsvDocument` または `ProjectSession` の再open） |
| #70 | Undo後の新規編集で旧redo系列を即時削除しない | 単体test（枝の保持） |
| #70 | 削除等の復元用blobを保存できる | 単体test |
| #70 | historyを削除してもCSVは通常CSVとして使える | 統合test |
| #70 | `.rowlyx` pack対象に含められる | `rowlyx` の往復test |
| #71 | tagへrollbackしても後続履歴が残る | 単体test |
| #71 | rollbackが履歴に記録され、さらにUndo / 編集できる | 統合test |
| #71 | tag情報がCSV本体へ混入しない | CSV bytes比較test |
| #243 | source A→Bのrow moveが1回のUndoで完全に戻る、Redoも1回 | 統合test |
| #243 | Undo途中失敗で片側だけの状態を公開しない | 失敗注入test |
| #243 | single-source Undo / Redoを壊さない | 既存testの回帰 |

Windows / Ubuntu CIの通過に加え、保存失敗とHEAD更新中断の挙動は、書込み失敗を注入するtestで確認してください。GUI実操作での確認は、tag / rollbackのGUI入口を追加する段階で行います。

## 未決事項

1. **保持上限の既定値と設定場所**
   - 選択肢: A) メモリ256を維持し、ディスクは無制限（GCまで） / B) ディスクにも既定上限を設ける。
   - 推奨: A。GCが非対象のため、上限で履歴を捨てるとtagの保証と矛盾します。設定場所は `.rwprj` に置きます。
2. **`ValidationRules` の履歴への扱い**
   - 選択肢: A) 永続履歴へ含める / B) 含めずsession限定のまま。
   - 推奨: B。検証規則はCSV保存で永続化されない仕様であり、永続履歴に載せると復元先の状態が曖昧になります。
3. **`.rwprj` に記録する項目**
   - 選択肢: A) 履歴ディレクトリ位置とschema versionのみ / B) HEAD idも記録する。
   - 推奨: A。HEADは `data/history/HEAD` が正本で、二重管理は不一致の原因になります。
4. **CSVが履歴HEADの状態と食い違った場合の扱い**
   - 選択肢: A) 新しい起点から始め旧履歴を残す / B) 利用者へ選択させる。
   - 推奨: A。対話なしCLIでも同じ結果になり、履歴を失いません。ただし、旧履歴をGUIで開く手段は別Issueとして必要です。
