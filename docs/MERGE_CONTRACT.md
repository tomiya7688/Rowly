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

### 分岐後のCode適用

~~~text
Base  = Code編集開始時のcanonical スナップショット
Local = parsed Code buffer
相手側 = 現在のcanonical document
~~~

ここで「相手側」は、ローカル変更と比較する現在側の状態を指します。
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
├─ 統合候補
├─ ローカル側だけの変更
├─ 相手側だけの変更
├─ 両側の同一変更
├─ セル競合
├─ 構造競合
└─ 変更範囲
~~~

MergePlan作成後、適用方針層が適用するか判断します。

## セル競合判定表

同じ座標について:

| Base | Local | 相手側 | 判定 |
| --- | --- | --- | --- |
| A | A | A | 変更なし |
| A | B | A | ローカル側だけの変更 |
| A | A | C | 相手側だけの変更 |
| A | B | B | 両側の同一変更 |
| A | B | C | 競合 |

最後のケースでどちらかを後勝ちさせてはいけません。

## ヘッダー

CSV row 1はcanonical dataですが、logical schema判断に使われるため、外部同期の自動mergeでは保守的に扱います。

- Baseと片側だけheader変更: 構造 / schema変更
- 両側が同一headerへ変更: 両側の同一構造変更として扱える余地はある
- 両側が異なるheaderへ変更: 競合

1.0ではheader変更を自動cell mergeせず、構造競合へ倒してよいです。

安全性を優先します。

## 不定幅行

行ごとのfield数はcanonical CSV構造の一部です。

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

同じbaseline rowに対して両側で異なるrow shape変更がある場合は構造競合です。

## 行の識別

CSV自体に安定した行IDはありません。

そのためphysical CSVの外部mergeでは、表示行番号を永続identityとして扱いません。

1.0の自動構造mergeは、baselineとの対応を一意に証明できる場合だけ許可します。

- baseline rowが一意
- order対応が一意
- duplicate rowにより対応が曖昧でない

対応が曖昧なら推測しません。

~~~text
ambiguous -> 構造競合
~~~

Project logical tableの安定したsource_id / provenanceは、どのphysical sourceへ書くかのidentityです。
physical CSV内の行identity問題を魔法のように解決するものではありません。

## 行削除

### ローカル削除 / 相手側変更なし

削除を採用できます。

### ローカル変更なし / 相手側削除

相手側 削除を採用できます。

### ローカル削除 / 相手側が同じ行を編集

競合です。

削除を優先して相手側 編集を捨てたり、編集を優先してLocal 削除を消したりしません。

### ローカル編集 / 相手側削除

同様に競合です。

## 行追加

baseline row間の挿入位置を基準に扱います。

### 片側だけ追加

安全に採用できます。

### 両側が同じ挿入位置へ同一row列を追加

同じ変更として1回だけ採用できます。

### 両側が同じ挿入位置へ異なるrowを追加

順序の意図を推測できないため構造競合です。

「Localを先、相手側を後」のような暗黙順序を作りません。

## 行並べ替え

明示的なstable 行identityが無いphysical CSVでは、並べ替えと削除+追加を安全に区別できない場合があります。

1.0では曖昧な並べ替えを自動mergeしません。

構造競合へ倒します。

## 競合の表現

### CellConflict

~~~text
cell
base
local
other
~~~

### StructuralConflict

最低限reasonと3 スナップショットを保持します。

理由の例:

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

統合候補をcanonicalへ適用します。

相手側由来の変更は外部baselineとして扱い、ローカル側だけの変更はRowly local 編集として再適用します。

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
- 競合情報を保持
- canonical currentは変更しない
- 利用者が解決して再Apply

将来「非競合だけ適用」を追加する場合は別Commandにします。

## 履歴

### 外部同期

disk側変更そのものを通常のLocal Undo commandにはしません。

外部同期後にreapplyしたLocal変更はUndo可能にします。

現在のメモリ履歴実装で過去の細かなLocal historyを再基準化時に保持できない場合でも、Local値そのものを失ってはいけません。

将来のpersistent Git-like historyでは、external sync前後のbranch / nodeを失わない設計へ接続します。

### Code適用

成功したCode適用は1つのUndo単位です。

merge内部のcell数に応じて大量の別Undo stepへ分割しません。

## 入力規則

MergePlan作成と入力規則を分けます。

Code適用では統合候補に対して入力規則を検証し、違反が1件でもあればcanonicalへ適用しません。

外部同期では外部CSVの違反値を読み込み拒否しない既存方針を維持します。
外部由来入力規則 violationは診断情報として扱い、Local reapply側は通常入力規則を通します。

## 計算量 / 大規模CSV

第1段階では安全性を優先し、スナップショット比較がO(table size)でも許容します。

最適化時も判定意味を変えません。

将来:

- row fingerprint
- immutable スナップショット sharing
- changed-region hint
- 逐次差分

を導入してよいですが、曖昧なmergeを高速化のために自動採用しません。

## テスト一覧

最低限:

### Cell

- ローカル側だけの変更
- 相手側だけの変更
- 両側の同一変更
- 両側の異なる変更による競合

### Row

- 追加 one side
- same 追加 both
- competing 追加
- 削除 one side
- 削除 vs 編集
- ragged row 編集
- ヘッダー変更
- 重複行
- 並べ替え

### 適用方針

- 外部同期: Diskを現在値にしローカル競合を保持
- Code適用: 競合時は正本を変更しない
- Code適用: 非競合mergeを1回のUndo単位で適用
- 同一入力でmerge結果が決定的

この日本語版をRowlyの3-way merge正本仕様とします。
