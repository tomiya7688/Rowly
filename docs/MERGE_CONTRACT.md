# 3-way merge契約

## 目的

Rowlyは外部編集と共存します。

また、Code Editorはcanonical CSVとは別のDraftを持つため、Draft編集中にcanonical CSVが別経路から変化することがあります。

この2つはどちらも、Base / Local / 相手側 の3状態を比較する問題です。

Rowlyではmerge判定をpure coreへ集約し、外部同期とCode適用で同じ競合判定を使います。

## 絶対条件

1. **Localを失ってから競合判定しない**
2. **merge判定中にcanonical stateを変更しない**
3. **同じ入力なら同じMergePlanを返す**
4. **曖昧な構造変更は自動推測しない**
5. **ragged rowを勝手に矩形化しない**
6. **競合なしと証明できる変更だけ自動mergeする**

## 入力

~~~text
Base
Local
相手側
~~~

### 外部同期

~~~text
Base  = 最後に確認済みのdisk baseline
Local = Rowly内の現在canonical local
相手側 = 新しいdisk
~~~

### Code適用 after divergence

~~~text
Base  = Code編集開始時のcanonical snapshot
Local = parsed Code buffer
相手側 = 現在のcanonical document
~~~

ここで相手側は「相手側」という意味です。
外部同期ではdisk、Code適用ではcurrent canonicalです。

## 純粋なmerge判定部

merge判定部はmutationしません。

概念:

~~~rust
fn plan_merge(
    base: &TableSnapshot,
    local: &TableSnapshot,
    other: &TableSnapshot,
) -> MergePlan
~~~

出力:

~~~text
MergePlan
├─ merged candidate
├─ ローカル側だけの変更 changes
├─ 相手側だけの変更 changes
├─ identical concurrent changes
├─ cell 競合s
├─ structural 競合s
└─ changed scope
~~~

MergePlan作成後、policy layerが適用するか判断します。

## セル競合判定表

同じ座標について:

| Base | Local | 相手側 | 判定 |
| --- | --- | --- | --- |
| A | A | A | unchanged |
| A | B | A | ローカル側だけの変更 |
| A | A | C | 相手側だけの変更 |
| A | B | B | 両側の同一変更 |
| A | B | C | 競合 |

最後のケースでどちらかを後勝ちさせてはいけません。

## Header

CSV row 1はcanonical dataですが、logical schema判断に使われるため、外部同期の自動mergeでは保守的に扱います。

- Baseと片側だけheader変更: structure/schema change
- 両側が同一headerへ変更: identical structural changeとして扱える余地はある
- 両側が異なるheaderへ変更: 競合

1.0ではheader変更を自動cell mergeせず、structure 競合へ倒してよいです。

安全性を優先します。

## Ragged row

rowごとのfield数はcanonical CSV構造の一部です。

~~~text
a,b,c
1
2,3
~~~

を勝手に

~~~text
a,b,c
1,,
2,3,
~~~

へ変換しません。

merge判定部は各rowの長さを保持します。

同じbaseline rowに対して両側で異なるrow shape変更がある場合はstructural 競合です。

## 行の識別

CSV自体にstable row IDはありません。

そのためphysical CSVのexternal mergeでは、表示row numberを永続identityとして扱いません。

1.0の自動構造mergeは、baselineとの対応を一意に証明できる場合だけ許可します。

- baseline rowが一意
- order対応が一意
- duplicate rowにより対応が曖昧でない

対応が曖昧なら推測しません。

~~~text
ambiguous -> structural 競合
~~~

Project logical tableのstable source_id / provenanceは、どのphysical sourceへ書くかのidentityです。
physical CSV内のrow identity問題を魔法のように解決するものではありません。

## Row deletion

### ローカル削除 / 相手側変更なし

deleteを採用できます。

### ローカル変更なし / 相手側削除

相手側 deleteを採用できます。

### ローカル削除 / 相手側が同じ行を編集

競合です。

削除を優先して相手側 editを捨てたり、editを優先してLocal deleteを消したりしません。

### ローカル編集 / 相手側削除

同様に競合です。

## 行追加

baseline row間のslotを基準に扱います。

### 片側だけinsert

安全に採用できます。

### 両側が同じslotへ同一row列をinsert

同じ変更として1回だけ採用できます。

### 両側が同じslotへ異なるrowをinsert

順序の意図を推測できないためstructural 競合です。

「Localを先、相手側を後」のような暗黙順序を作りません。

## 行並べ替え

明示的なstable row identityが無いphysical CSVでは、reorderとdelete+insertを安全に区別できない場合があります。

1.0では曖昧なreorderを自動mergeしません。

structure 競合へ倒します。

## 競合の表現

### CellConflict

~~~text
cell
base
local
other
~~~

### StructuralConflict

最低限reasonと3 snapshotを保持します。

reason例:

- HeaderChanged
- RowShapeChanged
- RowOrderChanged
- DuplicateRowsAmbiguous
- DeleteVsEdit
- CompetingInsertion
- AmbiguousMapping

ConflictDraftのlifecycleは SESSION_STATE_TRANSITIONS.md に従います。

## 適用方針: 外部同期

外部同期は、相手側=diskを現在状態として優先する既存方針を維持します。

### 競合なし

merged candidateをcanonicalへ適用します。

相手側由来の変更はexternal baselineとして扱い、Local-only変更はRowly local editとして再適用します。

### 競合あり

- Disk/相手側をcurrentへ採用してよい
- Local 競合を必ずConflictDraftへ保持
- 非競合Localを自動reapplyしてよい
- 競合 draftがあることをsession unresolved stateへ反映する

## 適用方針: Code適用

Code適用はatomicです。

### 競合なし

現在canonicalの相手側-only変更を保持したまま、Code Local変更を1つの編集transactionとして適用します。

UndoはCode適用由来の変更だけを戻し、途中で入った相手側/current変更は戻しません。

### 競合あり

Code由来の変更は一件もcanonicalへ適用しません。

- Code bufferを保持
- Conflict情報を保持
- canonical currentは変更しない
- 利用者が解決して再Apply

将来「非競合だけ適用」を追加する場合は別Commandにします。

## 履歴

### 外部同期

disk側変更そのものを通常のLocal Undo commandにはしません。

外部同期後にreapplyしたLocal変更はUndo可能にします。

現在のmemory history実装で過去の細かなLocal historyをrebasing時に保持できない場合でも、Local値そのものを失ってはいけません。

将来のpersistent Git-like historyでは、external sync前後のbranch / nodeを失わない設計へ接続します。

### Code適用

成功したApply Codeは1つのUndo単位です。

merge内部のcell数に応じて大量の別Undo stepへ分割しません。

## 入力規則

MergePlan作成とvalidationを分けます。

Code適用ではmerged candidateに対して入力規則を検証し、違反が1件でもあればcanonicalへ適用しません。

外部同期では外部CSVの違反値を読み込み拒否しない既存方針を維持します。
外部由来validation violationはdiagnosticとして扱い、Local reapply側は通常validationを通します。

## 計算量 / 大規模CSV

Phase 1では安全性を優先し、snapshot比較がO(table size)でも許容します。

最適化時も判定意味を変えません。

将来:

- row fingerprint
- immutable snapshot sharing
- changed-region hint
- streaming diff

を導入してよいですが、曖昧なmergeを高速化のために自動採用しません。

## テスト一覧

最低限:

### Cell

- ローカル側だけの変更
- 相手側だけの変更
- same concurrent
- different concurrent 競合

### Row

- insert one side
- same insert both
- competing insert
- delete one side
- delete vs edit
- ragged row edit
- header change
- duplicate rows
- reorder

### Policy

- 外部同期: Disk current + Local 競合 preserved
- Code適用: 競合時atomic no-op
- Code適用: non-競合 merge + one Undo
- repeated merge result determinism

この日本語版をRowlyの3-way merge正本仕様とします。
