# 差分・競合インスペクタの設計契約

## 目的

Rowlyは外部変更・Code Draft・Undo履歴・Project保存状態を扱うため、利用者が『何が違うのか』を確認できる共通の差分表示を持ちます。

差分表示自体は読み取り専用です。

競合解決などの変更操作は必ず通常のアプリケーションコマンドへ渡します。

## 比較対象

1. 現在の正本CSV vs 最後に保存した状態
2. 現在の正本CSV vs 現在のdisk
3. Code Draft vs Code開始時のbase
4. Code Draft vs 現在の正本CSV
5. 3-way競合のBase / Local / Other
6. Undo/Redo履歴の前後状態
7. 将来の永続履歴node間

## 差分の種類

```text
CellChanged
RowInserted
RowDeleted
RowMovedOrAmbiguous
ColumnInserted
ColumnDeleted
HeaderChanged
StructureChanged
```

CSVにstable row IDがない場合、行移動と削除+追加を無理に断定しません。

曖昧な場合は `RowMovedOrAmbiguous` 等の安全な表現にします。

## Cell差分

```text
DiffCell
├─ source identity
├─ CellRef / provenance
├─ before
├─ after
└─ kind
```

raw canonical valueを比較します。

表示形式が違うだけではcanonical diffにしません。

## 3-way競合表示

競合では2-way diffへ潰しません。

```text
Base
Local
Other
```

を同時に表示します。

例:

```text
A2
Base : old
Local: local-only
Disk : disk-only
```

## 複数source

logical tableの差分でも各変更はphysical source identityを保持します。

同じA2でもsourceが違えば別cellです。

## Filter / Sort

差分一覧は種類・source・column等で絞り込めます。

差分表示上のSort / FilterはView操作であり、元データを変更しません。

## navigation

次の変更 / 前の変更で移動できます。

変更を選ぶと:

- Editのphysical cell
- Viewの対応row
- Codeのtext span

へ移動できます。

移動先が現在存在しない場合は無理に別位置へ飛ばしません。

## 競合解決

インスペクタ内のボタンが直接tableを書き換えません。

例:

```text
ローカルを再適用
現在値を維持
手動編集へ移動
```

は通常のCommand Requestへ変換します。

解決前にStateStampを再確認します。

## 保存差分

保存前に『今回diskへ何が書かれるか』を表示できます。

SavePlanと同じStateStampを使い、古いpreviewをそのまま確定しません。

## Code差分

Code Draftがinvalid CSVでもtext diffは表示できます。

parse可能ならcell diffも併記します。

invalidな場合、誤ったcell mappingを作りません。

## 大規模CSV

全差分のbefore/after文字列を重複copyし続けないよう、source snapshotとrange参照を使える構造にします。

初期実装はO(n)比較でも構いませんが、UI描画はvirtualized listを前提にします。

## コマンド

```text
diff.open
diff.next
diff.previous
diff.filter
conflict.reapply_local
conflict.keep_current
```

全GUI操作はAltから到達可能にします。

## テスト

- 表示だけでCSV不変
- 2-way / 3-way差分
- ragged row差分
- duplicate rowで曖昧判定
- source identityを失わない
- stale previewを解決操作へ使わない
- Code invalid時にcell diffを捏造しない

この日本語版を差分・競合インスペクタの正本とします。