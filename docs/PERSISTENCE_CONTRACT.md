# 永続化 / 保存契約

## 目的

RowlyではCSVが正本です。

そのため「保存」は単に現在画面のbufferをfileへ書く操作ではありません。

現在のセッションには複数の物理CSV、project config、generated DSL、package archiveなど複数の永続化対象が存在し得ます。

この文書では、Save / Save As / Project Save / Pack / Exportの境界と、どこまでatomicを保証するかを定義します。

## 基本原則

1. **保存したと嘘をつかない**
2. **無損失かエラー**
3. **入力を誤って上書きしない**
4. **置換前に出力を完全に準備する**
5. **外部変更は上書き許可ではなく比較交換の失敗として扱う**
6. **複数成果物の部分成功を隠さない**

## 永続化成果物

保存対象をboolではなく成果物 identityで扱います。

~~~text
PersistenceArtifact
├─ CsvSource(source identity)
├─ ProjectManifest
├─ GeneratedConfig
├─ PackageArchive
└─ ExportOutput
~~~

各成果物は独立したdirty / backing generation / save resultを持ちます。

## Ctrl+S / file.save の意味

Rowlyの通常Saveは、**現在のセッションでpersist可能かつdirtyな成果物をすべて保存する**操作とします。

~~~text
file.save
  -> 現在のセッション dirty 成果物 set
  -> preflight
  -> save plan
  -> 成果物 results
~~~

Projectで複数CSVがdirtyなら、visible sourceだけを保存して「保存しました」とは表示しません。

logical row moveのように1操作がsource A / Bを同時にdirtyにした場合、通常Saveは両方を対象にします。

将来「現在sourceだけ保存」を提供する場合は別Commandにします。

~~~text
file.save
file.save_current_source
~~~

同じCommandを曖昧なcontext依存意味へしません。

## 保存範囲

概念:

~~~text
SaveScope
├─ Session
├─ CurrentSource   (将来optional)
├─ Project
└─ ExplicitArtifacts(...)
~~~

1.0のGUI通常SaveはSession scopeを基本とします。

CLIや内部APIは明示scopeを指定できます。

## 保存計画

Saveは直接writeを始めません。

~~~text
Session snapshot
  ↓
事前検査
  ↓
SavePlan
  ├─ 成果物
  ├─ expected backing generation
  ├─ target path
  ├─ overwrite policy
  └─ validation
  ↓
準備
  ↓
確定
  ↓
SaveReport
~~~

SavePlanはState Stampに紐付きます。

commit前にstampが変わっていた場合はplanを破棄して再preflightします。

## 事前検査

write開始前に少なくとも次を確認します。

- Pending Code draftがない
- inline editをcommitできる
- target pathが安全
- output aliasがない
- required parent directoryが存在/作成可能
- overwrite policy
- external backing generation
- target format limit
- project/package path safety
- required validation

preflightで検出できる失敗を、fileを作り始めてから検出しません。

## 準備

成果物ごとにtargetと同じfilesystem上のtemporary outputへ完全に書きます。

### CSV

- UTF-8
- LF
- ragged row保持
- writer completion
- flush
- sync_all
- 必要なら再parse validation

### .rowlyx

- temporary archiveへpack
- archive entry validation
- manifest validation
- path traversal / symlink safety
- package open validation

### XLSX Export

- target formatの表現可能性を事前検査
- long cell等でlossが起きる場合はerror
- temporary outputへ生成
- bridgeが成功を返しても必要なpost-validationを行う余地を持つ

準備成功前に既存targetを壊しません。

## 確定

### 比較交換

既存canonical 成果物のSaveは、prepare時に見たbacking generationをcommit直前に再確認します。

変わっていればreplaceしません。

~~~text
expected != current
=> ConcurrentModification
=> no overwrite
~~~

### 原子的置換

単一成果物は、platformが提供する安全なreplaceを使います。

既存fileをtruncateしてその場で書き始めません。

### ディレクトリ永続性

可能なplatformではreplace後にparent directory metadataのflushを検討します。

platform差はI/O adapterへ閉じ込め、application側へOS固有分岐を散らしません。

## Multi-成果物 Save

Session Saveは複数成果物を含み得ます。

### 実行時の原子性

全成果物を先に準備してから確定へ進みます。

これにより、2つ目の成果物が「書けない」と分かっているのに1つ目だけ先に確定することを減らします。

### クラッシュ時の原子性

通常のdirectory projectでは、複数file全体を1つのfilesystem transactionとしてcommitできるとは限りません。

1.0では次を正直に保証します。

- 成果物単位のatomic replace
- commit済み / 未commitをSaveReportへ正確に記録
- partial successを全体成功と表示しない
- retry時は未保存成果物だけを対象にできる

**複数成果物全体の電源断/crash atomicityは保証しません。**

.rowlyxはfinal archiveが1つの成果物なので、package完成後のarchive replaceをatomic boundaryにできます。

## 保存結果

~~~rust
struct SaveReport {
    scope: SaveScope,
    artifacts: Vec<ArtifactSaveReport>,
    pending: Vec<PendingWork>,
    conflicts: Vec<UnresolvedConflictId>,
    overall: SaveOverall,
}

struct ArtifactSaveReport {
    成果物: PersistenceArtifactId,
    result: SavePart,
}

enum SavePart {
    NotApplicable,
    Unchanged,
    Saved,
    Failed(String),
    Indeterminate(String),
}

enum SaveOverall {
    Complete,
    Partial,
    Blocked,
    Indeterminate,
}
~~~

### 状態不明

OS / filesystem errorやrollback failureにより、targetがbefore / afterのどちらかをRowlyが証明できない場合は Failed ではなく 状態不明 とします。

状態不明 成果物が1つでもある場合、SaveReport全体も状態不明です。

InteractiveSessionは SessionHealth::Uncertain へ移行し、通常Saveや破壊的操作を続行しません。

### 完了

- scope内のdirty persistable 成果物がすべてSaved/Unchanged
- blocking PendingWorkなし

ただしUnresolved Conflictやsession-only workが残る場合、セッション解決済みとは別です。

### 部分成功

一部成果物だけcommit済み。

UIは成功成果物と失敗成果物を区別して表示します。

部分成功は「状態不明」ではありません。
どの成果物がSavedで、どれがFailedかRowlyが把握できている状態です。

### 実行前停止

write前にPending Draft / validation / unsafe path等で止まった。

## 保存先と一致 と セッション解決済み

~~~text
保存先と一致
  = 永続化対象成果物がbackingと一致

セッション解決済み
  = 保存先と一致
    + Pending Draftなし
    + Unresolved Conflictなし
    + NotPersistable workなし
~~~

status barはこの2つを混同しません。

## Save As

### bare CSV

Save As成功時:

- canonical contentは同じ
- SourceInstanceIdは維持
- backing locator / backing generationを更新
- old fileは通常そのまま残す

### Project source

stable source_idを持つproject sourceのpath変更は、通常Save AsではなくProject MoveSource Commandを使います。

これによりmanifest、source identity、provenanceを同時に更新します。

## 上書き方針

### Save

既に開いているcanonical backingへのSaveはexpected generation付きreplaceです。

### Export / Pack

既存destinationを暗黙overwriteしません。

explicit OVERWRITE / confirmation等の明示操作を要求します。

### 入力先との同一実体

Export / Pack outputがinput集合とaliasする場合、overwrite許可があっても拒否します。

「overwriteを許可した」ことと「入力データを出力で破壊してよい」は別です。

## 同一実体の検出

既存pathはcanonical pathで比較します。

未作成outputは、安全に解決したparent directory + filenameとして比較します。

検査対象:

- direct source
- directory source配下で実際に収録するfile
- project manifest
- generated/user scripts
- package source
- symlink
- Windows junction / reparse point
- case-insensitive filesystem差

directory sourceでは**entry収集後に全entryとoutputを照合**します。

## 失敗時の扱い

### 準備 failure

targetは変更しません。

### 比較交換失敗

targetは変更しません。
external sync対象として報告します。

### 確定 failure before replace

targetは変更しません。

### 確定 failure after一部成果物成功

SaveReport=部分成功。

成功成果物を「失敗したこと」に戻して嘘をつきません。

個別operationが安全なrollbackを実装している場合はrollbackしてよいですが、rollback自体の失敗を隠しません。

rollback結果が確認できない場合は該当成果物を状態不明にします。

## 再試行

再試行は新しいSave commandです。

前回planをそのまま再利用せず、最新State Stamp / backing generationで再preflightします。

## Exportは保存ではない

XLSX / JSON / Markdown等へのExport成功はcanonical CSVのdirtyをcleanにしません。

Export outputは交換成果物であり、canonical backingではありません。

## PackはProject保存ではない

.rowlyxへのPackだけで、元directory projectのdirty CSV/configが自動的にsavedになったことにはしません。

Pack前にどのstateをpackageへ含めるかを明示し、staged/in-memory stateを勝手に無視しません。

ProjectSessionのpackage Saveは別途、managed workspaceをfinal archiveへcommitする正式なSave operationとして扱います。

## クラッシュ復旧との境界

この契約はdiskへcommitした成果物の安全性を扱います。

未保存memory stateのprocess crash recoveryは SESSION_STATE_TRANSITIONS.md の保証境界に従い、別のRecovery Store設計とします。

## テスト一覧

最低限:

- single CSV Save
- no-op Save
- external modification before Save
- external modification between prepare/commit
- dirty Code blocks Save
- inline edit validation failure blocks Save
- multiple dirty CSV sources
- CSV success + config failure -> 部分成功
- package prepare failure preserves original archive
- export output aliases direct input
- pack output aliases directory child input
- long XLSX cell -> 無損失かエラー
- retry after partial failure

この日本語版をRowlyのPersistence / Save正本仕様とします。
