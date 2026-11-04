# View表示・絞り込み・並べ替えの設計契約

## 目的

ViewはCSV正本を書き換えず、同じデータを目的に応じて見やすくするための層です。

Filter、Sort、列の並び、非表示、幅、固定、表示形式、同値セルの視覚結合をView設定として扱います。

**表示のためにCSVの行順・列順・値・空欄を変更しません。**

## 絶対条件

1. Filterで行を削除しない。
2. SortでCSVの行順を並べ替えない。
3. 列非表示でCSVの列を削除しない。
4. 列順変更でCSVの列順を変更しない。
5. 表示形式でraw valueを書き換えない。
6. 視覚結合で値を空欄化しない。
7. View上の行・セルから元のphysical source / row / cellへ必ず戻れる。

## Viewの処理順

```text
Canonical CSV / Logical Table
        ↓
行の参照元を解決
        ↓
Filter
        ↓
安定Sort
        ↓
列の表示順 / 非表示
        ↓
表示形式
        ↓
同値セルの視覚結合
        ↓
固定行・固定列 / viewport
        ↓
描画
```

FilterやSortは行のprovenanceを変えません。

## 行の識別

View上の表示行番号を永続identityにしません。

各表示行は少なくとも次へ戻れる必要があります。

```text
ViewRow
├─ logical table identity
├─ source_id / SourceInstanceId
├─ physical record identity
└─ current raw row
```

SortやFilterで表示位置が変わっても、同じphysical rowを指し続けます。

## Filter

Filterは三値で評価します。

```text
一致
不一致
未解決
```

`未解決` は、参照列が消えた、derived resultがstale/error、型解釈に失敗した等です。

既定では未解決行を黙って隠しません。別枠または警告付きで表示します。

### 1.0のFilter対象

- raw文字列
- 明示された列型によるNumber / Boolean等の解釈
- freshなCalculation derived result

表示用にformat済みの文字列だけを根拠にFilterしません。

### 比較

raw文字列では完全一致を基本とし、Contains / StartsWith / EndsWith等を明示できます。

数値比較は列が明示的に数値として解釈される場合だけ行います。暗黙の型推論でFilter結果を変えません。

## Sort

Sortはstable sortとします。

同順位の行は入力順、最終的にはprovenance順を維持し、再描画ごとに順番が揺れないようにします。

複数キーを持てます。

```text
1. status ASC
2. date DESC
3. provenance stable
```

Sort対象もFilterと同様、raw / 明示型 / fresh derived resultを区別します。

stale/errorのderived resultは通常値と混ぜず、未解決グループとして末尾へ安定配置します。

## 列表示

Viewでは次を設定できます。

- 列の表示 / 非表示
- 表示順
- 幅
- 左側固定
- 表示名
- 表示形式

これらはCSV構造を変更しません。

列のidentityは、projectでは一意header等のsemantic identityを優先し、列順変更で設定が別列へ移らないようにします。

参照列が消えた場合は設定を削除せずdormantにし、#69の再解決契約へ従います。

## 固定行・固定列

固定は描画上のviewport設定です。

1.0では先頭N行 / 左N列を基本にします。

固定のために行や列を複製しません。

## 名前付きView

一連のView設定を名前付きで保存できます。

```text
ViewProfile
├─ name
├─ filters
├─ sort_keys
├─ column_layout
├─ formatting
├─ grouping
└─ freeze
```

bare CSVではsession-onlyです。

Projectではgenerated init DSLへ保存し、user.rlyを上書きしません。

### active View

Projectは複数の名前付きViewを持てますが、active ViewはUI状態です。

active Viewをprojectへ保存するかは設定として明示し、CSV正本とは無関係です。

## ViewとEditの関係

1.0ではViewのFilter / SortはViewだけに適用します。

Editへ切り替えたとき、View上の表示順をそのままCSV行順として扱いません。

Viewで選択した行からEditへ移動する場合はprovenanceを使って元のcanonical rowへ移動します。

将来『Viewの並びをデータへ反映』を提供する場合は、明示的なデータ変更Commandとして別機能にします。

## コピー

曖昧にしません。

- raw CSVをコピー
- Viewに表示している文字列をコピー

を別Commandとして扱います。

表示形式を適用した値をコピーしただけでCSV値が変わったことにはしません。

## コマンド

例:

```text
view.filter.add
view.filter.clear
view.sort.add
view.sort.clear
view.column.hide
view.column.show
view.column.move
view.column.width.set
view.freeze.set
view.profile.activate
view.profile.save
view.profile.delete
```

GUIに出す操作は全てAlt KeyTipsから到達可能にします。

## 永続化

View設定はCSV外です。

Project内では #60 のgenerated init DSLを使います。

保存失敗時はstagingを保持し、CSV保存成功とView設定保存成功を混同しません。

## 外部変更

外部変更後は設定を全resetしません。

source / header / logical identityごとに再解決し、壊れた項目だけdormantへ移します。

## 性能

Filter / Sortは大規模CSVでも全コピーを前提にしません。

初期実装ではrow index/provenanceの配列をView側に作り、canonical rowsを複製しない構造を優先します。

## テスト

- FilterでCSV行数・値が変わらない
- SortでCSV行順が変わらない
- 複数キーSortがstable
- stale derived filterが行を黙って消さない
- 列非表示 / 並べ替え / 幅変更でCSV不変
- View選択からphysical provenanceへ戻れる
- project再openで名前付きViewを復元
- missing columnだけdormantになり他設定を維持

この日本語版をView表示契約の正本とします。