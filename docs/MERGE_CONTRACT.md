# 3-way Merge 契約

## 目的

Rowlyは外部編集と共存します。

また、Code Editorはcanonical CSVとは別のDraftを持つため、Draft編集中にcanonical CSVが別経路から変化することがあります。

この2つはどちらも、Base / Local / Other の3状態を比較する問題です。

Rowlyではmerge判定をpure coreへ集約し、External SyncとCode Applyで同じ競合判定を使います。

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
Other
~~~

### External Sync

~~~text
Base  = 最後に確認済みのdisk baseline
Local = Rowly内の現在canonical local
Other = 新しいdisk
~~~

### Code Apply after divergence

~~~text
Base  = Code編集開始時のcanonical snapshot
Local = parsed Code buffer
Other = 現在のcanonical document
~~~

ここでOtherは「相手側」という意味です。
External Syncではdisk、Code Applyではcurrent canonicalです。

## Pure Merge Core

merge coreはmutationしません。

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
├─ local-only changes
├─ other-only changes
├─ identical concurrent changes
├─ cell conflicts
├─ structural conflicts
└─ changed scope
~~~

MergePlan作成後、policy layerが適用するか判断します。

## Cell truth table

同じ座標について:

| Base | Local | Other | 判定 |
| --- | --- | --- | --- |
| A | A | A | unchanged |
| A | B | A | local-only |
| A | A | C | other-only |
| A | B | B | same concurrent change |
| A | B | C | conflict |

最後のケースでどちらかを後勝ちさせてはいけません。

## Header

CSV row 1はcanonical dataですが、logical schema判断に使われるため、External Syncの自動mergeでは保守的に扱います。

- Baseと片側だけheader変更: structure/schema change
- 両側が同一headerへ変更: identical structural changeとして扱える余地はある
- 両側が異なるheaderへ変更: conflict

1.0ではheader変更を自動cell mergeせず、structure conflictへ倒してよいです。

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

merge coreは各rowの長さを保持します。

同じbaseline rowに対して両側で異なるrow shape変更がある場合はstructural conflictです。

## Row identity

CSV自体にstable row IDはありません。

そのためphysical CSVのexternal mergeでは、表示row numberを永続identityとして扱いません。

1.0の自動構造mergeは、baselineとの対応を一意に証明できる場合だけ許可します。

- baseline rowが一意
- order対応が一意
- duplicate rowにより対応が曖昧でない

対応が曖昧なら推測しません。

~~~text
ambiguous -> structural conflict
~~~

Project logical tableのstable source_id / provenanceは、どのphysical sourceへ書くかのidentityです。
physical CSV内のrow identity問題を魔法のように解決するものではありません。

## Row deletion

### Local delete / Other unchanged

deleteを採用できます。

### Local unchanged / Other delete

Other deleteを採用できます。

### Local delete / Other edits same row

conflictです。

削除を優先してOther editを捨てたり、editを優先してLocal deleteを消したりしません。

### Local edit / Other delete

同様にconflictです。

## Row insertion

baseline row間のslotを基準に扱います。

### 片側だけinsert

安全に採用できます。

### 両側が同じslotへ同一row列をinsert

同じ変更として1回だけ採用できます。

### 両側が同じslotへ異なるrowをinsert

順序の意図を推測できないためstructural conflictです。

「Localを先、Otherを後」のような暗黙順序を作りません。

## Row reorder

明示的なstable row identityが無いphysical CSVでは、reorderとdelete+insertを安全に区別できない場合があります。

1.0では曖昧なreorderを自動mergeしません。

structure conflictへ倒します。

## Conflict representation

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

## Policy: External Sync

External Syncは、Other=diskを現在状態として優先する既存方針を維持します。

### conflictなし

merged candidateをcanonicalへ適用します。

Other由来の変更はexternal baselineとして扱い、Local-only変更はRowly local editとして再適用します。

### conflictあり

- Disk/Otherをcurrentへ採用してよい
- Local conflictを必ずConflictDraftへ保持
- 非競合Localを自動reapplyしてよい
- conflict draftがあることをsession unresolved stateへ反映する

## Policy: Code Apply

Code Applyはatomicです。

### conflictなし

現在canonicalのOther-only変更を保持したまま、Code Local変更を1つの編集transactionとして適用します。

UndoはCode Apply由来の変更だけを戻し、途中で入ったOther/current変更は戻しません。

### conflictあり

Code由来の変更は一件もcanonicalへ適用しません。

- Code bufferを保持
- Conflict情報を保持
- canonical currentは変更しない
- 利用者が解決して再Apply

将来「非競合だけ適用」を追加する場合は別Commandにします。

## History

### External Sync

disk側変更そのものを通常のLocal Undo commandにはしません。

External Sync後にreapplyしたLocal変更はUndo可能にします。

現在のmemory history実装で過去の細かなLocal historyをrebasing時に保持できない場合でも、Local値そのものを失ってはいけません。

将来のpersistent Git-like historyでは、external sync前後のbranch / nodeを失わない設計へ接続します。

### Code Apply

成功したApply Codeは1つのUndo単位です。

merge内部のcell数に応じて大量の別Undo stepへ分割しません。

## Validation

MergePlan作成とvalidationを分けます。

Code Applyではmerged candidateに対して入力規則を検証し、違反が1件でもあればcanonicalへ適用しません。

External Syncでは外部CSVの違反値を読み込み拒否しない既存方針を維持します。
外部由来validation violationはdiagnosticとして扱い、Local reapply側は通常validationを通します。

## Complexity / large CSV

Phase 1では安全性を優先し、snapshot比較がO(table size)でも許容します。

最適化時も判定意味を変えません。

将来:

- row fingerprint
- immutable snapshot sharing
- changed-region hint
- streaming diff

を導入してよいですが、曖昧なmergeを高速化のために自動採用しません。

## Test matrix

最低限:

### Cell

- local-only
- other-only
- same concurrent
- different concurrent conflict

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

- External Sync: Disk current + Local conflict preserved
- Code Apply: conflict時atomic no-op
- Code Apply: non-conflict merge + one Undo
- repeated merge result determinism

この日本語版をRowlyの3-way merge正本仕様とします。
