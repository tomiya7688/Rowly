# Persistence / Save 契約

## 目的

RowlyではCSVが正本です。

そのため「保存」は単に現在画面のbufferをfileへ書く操作ではありません。

current sessionには複数のphysical CSV、project config、generated DSL、package archiveなど複数のpersist対象が存在し得ます。

この文書では、Save / Save As / Project Save / Pack / Exportの境界と、どこまでatomicを保証するかを定義します。

## 基本原則

1. **Save never lies**
2. **lossless-or-error**
3. **never overwrite an input by accident**
4. **prepare before replace**
5. **external modification is compare-and-swap failure, not permission to overwrite**
6. **multi-artifact partial successを隠さない**

## Persistence Artifact

保存対象をboolではなくartifact identityで扱います。

~~~text
PersistenceArtifact
├─ CsvSource(source identity)
├─ ProjectManifest
├─ GeneratedConfig
├─ PackageArchive
└─ ExportOutput
~~~

各artifactは独立したdirty / backing generation / save resultを持ちます。

## Ctrl+S / file.save の意味

Rowlyの通常Saveは、**current sessionでpersist可能かつdirtyなartifactをすべて保存する**操作とします。

~~~text
file.save
  -> current session dirty artifact set
  -> preflight
  -> save plan
  -> artifact results
~~~

Projectで複数CSVがdirtyなら、visible sourceだけを保存して「保存しました」とは表示しません。

logical row moveのように1操作がsource A / Bを同時にdirtyにした場合、通常Saveは両方を対象にします。

将来「現在sourceだけ保存」を提供する場合は別Commandにします。

~~~text
file.save
file.save_current_source
~~~

同じCommandを曖昧なcontext依存意味へしません。

## Save Scope

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

## Save Plan

Saveは直接writeを始めません。

~~~text
Session snapshot
  ↓
Preflight
  ↓
SavePlan
  ├─ artifact
  ├─ expected backing generation
  ├─ target path
  ├─ overwrite policy
  └─ validation
  ↓
Prepare
  ↓
Commit
  ↓
SaveReport
~~~

SavePlanはState Stampに紐付きます。

commit前にstampが変わっていた場合はplanを破棄して再preflightします。

## Preflight

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

## Prepare

artifactごとにtargetと同じfilesystem上のtemporary outputへ完全に書きます。

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

Prepare成功前に既存targetを壊しません。

## Commit

### Compare-and-swap

既存canonical artifactのSaveは、prepare時に見たbacking generationをcommit直前に再確認します。

変わっていればreplaceしません。

~~~text
expected != current
=> ConcurrentModification
=> no overwrite
~~~

### Atomic replace

単一artifactは、platformが提供する安全なreplaceを使います。

既存fileをtruncateしてその場で書き始めません。

### Directory durability

可能なplatformではreplace後にparent directory metadataのflushを検討します。

platform差はI/O adapterへ閉じ込め、application側へOS固有分岐を散らしません。

## Multi-artifact Save

Session Saveは複数artifactを含み得ます。

### Runtime atomicity

全artifactを先にPrepareしてからCommitへ進みます。

これにより、2つ目のartifactが「書けない」と分かっているのに1つ目だけ先に確定することを減らします。

### Crash atomicity

通常のdirectory projectでは、複数file全体を1つのfilesystem transactionとしてcommitできるとは限りません。

1.0では次を正直に保証します。

- artifact単位のatomic replace
- commit済み / 未commitをSaveReportへ正確に記録
- partial successを全体成功と表示しない
- retry時は未保存artifactだけを対象にできる

**複数artifact全体の電源断/crash atomicityは保証しません。**

.rowlyxはfinal archiveが1つのartifactなので、package完成後のarchive replaceをatomic boundaryにできます。

## Save Report

~~~rust
struct SaveReport {
    scope: SaveScope,
    artifacts: Vec<ArtifactSaveReport>,
    pending: Vec<PendingWork>,
    conflicts: Vec<UnresolvedConflictId>,
    overall: SaveOverall,
}

enum SaveOverall {
    Complete,
    Partial,
    Blocked,
}
~~~

### Complete

- scope内のdirty persistable artifactがすべてSaved/Unchanged
- blocking PendingWorkなし

ただしUnresolved Conflictやsession-only workが残る場合、SessionResolvedとは別です。

### Partial

一部artifactだけcommit済み。

UIは成功artifactと失敗artifactを区別して表示します。

### Blocked

write前にPending Draft / validation / unsafe path等で止まった。

## BackingClean と SessionResolved

~~~text
BackingClean
  = persist対象artifactがbackingと一致

SessionResolved
  = BackingClean
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

## Overwrite Policy

### Save

既に開いているcanonical backingへのSaveはexpected generation付きreplaceです。

### Export / Pack

既存destinationを暗黙overwriteしません。

explicit OVERWRITE / confirmation等の明示操作を要求します。

### Input alias

Export / Pack outputがinput集合とaliasする場合、overwrite許可があっても拒否します。

「overwriteを許可した」ことと「入力データを出力で破壊してよい」は別です。

## Alias detection

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

## Failure semantics

### Prepare failure

targetは変更しません。

### CAS failure

targetは変更しません。
external sync対象として報告します。

### Commit failure before replace

targetは変更しません。

### Commit failure after一部artifact成功

SaveReport=Partial。

成功artifactを「失敗したこと」に戻して嘘をつきません。

個別operationが安全なrollbackを実装している場合はrollbackしてよいですが、rollback自体の失敗を隠しません。

## Retry

Retryは新しいSave commandです。

前回planをそのまま再利用せず、最新State Stamp / backing generationで再preflightします。

## ExportはSaveではない

XLSX / JSON / Markdown等へのExport成功はcanonical CSVのdirtyをcleanにしません。

Export outputは交換成果物であり、canonical backingではありません。

## PackはProject Saveではない

.rowlyxへのPackだけで、元directory projectのdirty CSV/configが自動的にsavedになったことにはしません。

Pack前にどのstateをpackageへ含めるかを明示し、staged/in-memory stateを勝手に無視しません。

ProjectSessionのpackage Saveは別途、managed workspaceをfinal archiveへcommitする正式なSave operationとして扱います。

## Crash Recoveryとの境界

この契約はdiskへcommitしたartifactの安全性を扱います。

未保存memory stateのprocess crash recoveryは SESSION_STATE_TRANSITIONS.md の保証境界に従い、別のRecovery Store設計とします。

## Test matrix

最低限:

- single CSV Save
- no-op Save
- external modification before Save
- external modification between prepare/commit
- dirty Code blocks Save
- inline edit validation failure blocks Save
- multiple dirty CSV sources
- CSV success + config failure -> Partial
- package prepare failure preserves original archive
- export output aliases direct input
- pack output aliases directory child input
- long XLSX cell -> lossless-or-error
- retry after partial failure

この日本語版をRowlyのPersistence / Save正本仕様とします。
