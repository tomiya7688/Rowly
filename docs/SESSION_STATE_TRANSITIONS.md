# セッション状態・変更適用・保存の設計契約

## 目的

RowlyではCSVが正本です。

しかし、実際の操作中にはCSV正本以外にも、未適用Code、セル編集中の値、Calculation Binding、derived result、Project設定、外部disk状態など複数の状態が同時に存在します。

これらをGUI handlerごとに個別同期すると、同じ操作でも入口によって挙動が変わり、保存漏れ・外部変更の上書き・古い計算結果表示が発生します。

この文書では、Rowlyのinteractive sessionが守る状態遷移の共通契約を定義します。

## 絶対条件

### 1. Local work never disappears silently

Rowly内で作られた未保存・未適用の作業は、明示的なDiscardなしに消してはなりません。

対象には少なくとも次を含みます。

- canonical CSVの未保存編集
- Text Editorの未適用Code
- inline cell editの未確定値
- Calculation Bindingの未保存変更
- Project manifest / generated configのstaged変更
- .rowlyx workspaceの未pack変更
- merge conflictとして退避したLocal変更

外部変更、Open、source切替、Save、Close、Quitのどの経路でもこの原則を破ってはなりません。

### 2. Save never lies

「保存しました」と表示するのは、保存対象として扱った状態が実際に保存済みになったときだけです。

未適用Codeを無視してCSVだけ保存した場合に「保存しました」と表示してはなりません。

複数の保存対象がある場合は、結果を分けて扱います。

~~~text
CSV: saved
Project config: failed
Package: not updated
~~~

この場合は全体成功ではありません。

### 3. Evaluated means provably fresh

Calculation resultの Evaluated は「以前計算に成功した」という意味ではありません。

**現在のcanonical CSVと現在のCalculation Bindingに対して正しいことを確認できる場合だけ Evaluated とします。**

古いdependency snapshotを持つresultを、handlerの再計算漏れだけで Evaluated 表示できてはなりません。

### 4. Every canonical mutation emits one ChangeSet

canonical CSVを変更したすべての操作は、変更内容を共通の DocumentChangeSet として上位層へ報告します。

変更元がGUIか、Codeか、外部同期か、Undoか、DSLか、Luauかで契約を変えません。

### 5. Open / Switch / Close / Quit use one Transition Guard

現在のsessionを捨てる可能性がある操作は、同じ未保存判定を使います。

- Open CSV
- Open Project
- Project source switch
- Close
- Quit
- 将来のworkspace replacement

個別handlerで独自のdirty判定を書いてはなりません。

---

## 状態を5つに分ける

~~~text
Interactive Session
├─ Canonical Data
├─ Drafts
├─ Rules / Config
├─ Derived State
└─ Backing / Persistence
~~~

### Canonical Data

CsvDocument が保持する、現在確定済みのCSV表です。

Edit / Code Apply / DSL / Luau / Undo / Redo / External Sync等で変更されます。

ここへ入った変更は通常のhistory / validation / dirty contractに従います。

### Drafts

まだcanonical CSVへ適用していないユーザー作業です。

~~~text
Drafts
├─ InlineCellDraft
└─ CodeDraft
~~~

Draftは CsvDocument::is_dirty() と別物です。

Codeを編集しただけではcanonical CSVはdirtyではありませんが、**作業は失ってはいけません**。

したがって dirty == false を「捨ててもよい」と解釈してはいけません。

### Rules / Config

CSV外に存在し、保存対象になる設定です。

- Calculation Binding
- Validation
- Viewer config
- Project manifest
- generated init DSL
- package workspace

Calculation Bindingのrule変更とderived resultは別状態です。

~~~text
Calculation State
├─ Rules     -> persist対象 / dirtyになり得る
└─ Results   -> derived cache / dirtyとは呼ばない
~~~

### Derived State

canonical CSVとRulesから再生成できる状態です。

- Calculation result
- View interpretation
- 将来のindex / cache

Derived Stateは正本ではありません。

保存対象のdirtyとは区別し、Fresh / Stale / Pending / Error で管理します。

### Backing / Persistence

diskやproject packageなど、sessionの外側にある保存先です。

- CSV file + fingerprint
- .rwprj
- .rowlyx archive
- staged project workspace

---

## Pending Work と Dirty を分ける

一つのbool dirty へ押し込めません。

概念上は次のsummaryを持ちます。

~~~rust
struct SessionWorkState {
    pending: PendingWorkState,
    persistent: PersistentDirtyState,
}

struct PendingWorkState {
    inline_cell: bool,
    code: CodeDraftStatus,
}

struct PersistentDirtyState {
    csv: bool,
    calculation_rules: bool,
    project_config: bool,
    package: bool,
}
~~~

実際のTransition Guardでは、さらに次を独立categoryとして扱います。

~~~text
SessionWorkSummary
├─ Pending Drafts
├─ Persistent Dirty
├─ Unresolved Conflicts
└─ Session-only / Not Persistable
~~~

### Unresolved Conflicts

external mergeやCode Applyで退避したLocal conflictは、canonical CSVがcleanでも未解決作業です。

conflict draftが残っている状態を「全部保存済み」と表示してはなりません。

### Session-only / Not Persistable

bare CSVのCalculation Bindingなど、現在のbackingへ保存できない状態です。

通常Saveで消えないため、Open / Close / Quit時はProject化・Discard・Cancel等の明示的なresolutionが必要です。

### CodeDraftStatus

最低限:

~~~text
Clean
Dirty
Diverged
Conflict
~~~

- Dirty: Code bufferがbase snapshotから変更された
- Diverged: Code編集中にcanonical CSVが別経路から変わった
- Conflict: Apply時の3-way mergeで競合した

外部変更が起きてもdirty Code bufferを自動で上書きしません。

---

## Document Revision

CsvDocument はdisk fingerprintとは別に、session内で単調増加する DocumentRevision を持ちます。

revisionはcanonical tableの意味が変わるたびに増えます。

対象:

- cell edit
- row / column edit
- replace contents
- Apply Code
- Undo / Redo
- external sync
- save中に発生したexternal merge
- Materialize
- DSL / Luauによるcanonical edit

disk saveだけで内容が変わらない場合はrevisionを増やしません。

DocumentRevision は「保存されたか」ではなく、「session内のcanonical contentが変化したか」を表します。

---

## DocumentChangeSet

canonical mutationは共通のreceiptを返します。

概念:

~~~rust
struct DocumentChangeSet {
    origin: ChangeOrigin,
    before_revision: DocumentRevision,
    after_revision: DocumentRevision,
    scope: ChangeScope,
    conflicts: Vec<MergeConflict>,
    disk_baseline_changed: bool,
    history_effect: HistoryEffect,
}

enum ChangeScope {
    Cells(BTreeSet<CellRef>),
    Structure,
    WholeTable,
}

enum ChangeOrigin {
    Edit,
    CodeApply,
    ExternalSync,
    SaveSync,
    Undo,
    Redo,
    Dsl,
    Luau,
    Import,
    Materialize,
}

enum HistoryEffect {
    None,
    Recorded,
    ResetToDisk,
}
~~~

正確なcell集合を出せない操作は WholeTable を返して構いません。

**分からないから通知しない、は禁止です。**

上位層は ChangeSet を受けてderived stateやDraftを更新します。

---

## 変更適用パイプライン

canonical dataを変える操作は、入口に関係なく次を通ります。

~~~text
Command
  ↓
Preflight
  ↓
Canonical Mutation
  ↓
DocumentChangeSet
  ↓
Session Reconcile
  ├─ Draft relation update
  ├─ Calculation invalidate / recalculate
  ├─ View/cache invalidate
  ├─ selection/bounds reconcile
  └─ status/result construction
~~~

GUI handlerが直接、document変更・calculation再計算・buffer更新・status設定を個別に並べてはいけません。

後処理を忘れる入口が必ず生まれるためです。

---

## Session Reconcile

DocumentChangeSet を受けた後処理を一箇所へ集めます。

### Draft

CodeDraftがCleanならcanonical textへ同期してよい。

CodeDraftがDirtyならbufferを上書きしません。

base revisionとcurrent revisionが違えば Diverged にします。

### Calculation

ChangeScope::Cells なら依存graphから影響するbindingだけを更新します。

Structure / WholeTable なら安全側へ倒し、全bindingを再評価またはstale化します。

### Viewer / cache

canonical revisionに依存するcacheは無効化します。

### Selection

row/column削除等で選択位置が範囲外になった場合だけclampします。

---

## Calculation resultは自己検証する

event通知だけに正しさを依存させません。

現在の CalculationResult はdependency snapshotを保持しています。

表示・Materialize時には少なくとも次を検証します。

- resultのbinding revisionが現在のbinding revisionと一致
- dependency cellのraw valueがsnapshotと一致
- dependency先にbindingがある場合、そのbinding revisionが一致
- dependency先derived resultのrevisionが一致

一致しなければstored statusが Evaluated でも、**effective statusはStale** とします。

概念:

~~~text
engine.result_for(document, id)
    -> FreshEvaluated(value)
    -> Stale(previous_value)
    -> Pending
    -> Error(...)
~~~

GUIは内部の CalculationResult.status を直接信用せず、current documentに対するeffective resultを取得します。

Materializeも FreshEvaluated だけを受け付けます。

これにより、handlerが再計算を忘れても古い値を正常結果として確定できません。

---

## 3-way mergeを一つにする

外部同期とCode Applyで別々のmerge logicを持ちません。

pureなtable merge coreへ集約します。

~~~text
Base
Local
Current / Remote
  ↓
TableMerge
  ↓
MergeOutcome
├─ merged
├─ changed cells
├─ conflicts
└─ structural conflict
~~~

利用箇所:

- External Sync: Base=disk baseline / Local=Rowly canonical local / Remote=new disk
- Apply Code after divergence: Base=Code編集開始時snapshot / Local=parsed Code buffer / Current=現在canonical document

### 競合の絶対条件

同じcellをLocalとCurrent/RemoteがBaseから別値へ変更した場合、Localを失ってはなりません。

~~~text
Base  = old
Local = local-only
Other = disk-only

=> conflict
=> current policyでOtherをcurrentにしてよい
=> Localは必ずdraftへ残す
~~~

merge前にLocalを破棄してから競合判定してはいけません。

### External Sync

非競合変更は自動mergeしてよい。

競合はDisk/currentを現在値にし、Localをconflict draftへ退避する既存方針を維持します。

### Draft baseline

CodeDraftとInlineCellDraftは、作業開始時のbaseを持ちます。

CodeDraft:

~~~text
base revision
base table snapshot
current text
draft status
~~~

InlineCellDraft:

~~~text
target cell
base revision
base raw value
current edit value
~~~

canonical CSVが別経路で変わってもdraft text/valueは上書きしません。

Inline cellのtarget自体がBaseから変化した場合は、commit時に競合として扱います。

### Code Apply

Codeはユーザーが明示的にApplyする一つの編集単位です。

- base revisionとcurrent revisionが同じなら通常Apply
- divergedなら3-way merge
- conflictが1件でもあれば、Code由来の変更は**一件もcanonicalへ適用しない**
- Code bufferをそのまま保持する
- conflict情報を表示し、解決後に再Applyする
- 競合解決前に「Apply成功」と表示しない

External Syncは非競合部分を自動mergeしてよい一方、Code Applyはatomicに扱います。

理由は、Apply Codeが利用者の明示した1つの編集transactionだからです。

部分適用して「どこまで入ったか」を別状態にするより、全件成功か全件未適用かを維持します。

将来、明示的な「非競合部分だけ適用」を追加する場合は別Commandとして扱います。

---

## Transition Guard

sessionを置き換える操作は、共通の TransitionGuard を通します。

対象:

- Open CSV
- Open Project
- Switch Source
- Close
- Quit

判定対象:

- inline cell draft
- Code draft
- CSV dirty
- Calculation rule dirty
- Project dirty
- Package dirty
- session-only rule

### bare CSVのsession-only rule

bare CSVではCalculation Binding等を自動sidecar保存しません。

そのためsession-only ruleがある場合、通常Saveでは保存できません。

この場合は少なくとも次を選べる状態として扱います。

- Project化して保存
- Discard
- Cancel

Open / Close / Quitごとに別の確認ロジックを作りません。

### Transitionは二段階でcommitする

Open / source switchは、現在sessionを先に破棄しません。

~~~text
1. Guard
2. User resolution
3. Prepare next session
4. Validate next session
5. Commit swap
6. Dispose old session
~~~

新しいCSV / Projectのopen、Code text生成、Calculation Binding復元、watcher準備の途中で失敗した場合は、現在sessionをそのまま維持します。

「古いsessionを消してから新しいsessionをopenする」順序は禁止します。

---

## Save Coordinator

Saveを単一のbool結果にしません。

概念:

~~~rust
struct SaveReport {
    csv: SavePart,
    project_config: SavePart,
    package: SavePart,
    pending: Vec<PendingWork>,
}

enum SavePart {
    NotApplicable,
    Unchanged,
    Saved,
    Failed(String),
}
~~~

### Save前

- active inline editがある場合は、通常のcell validationを通してcommitを試みる
- inline editのcommitに失敗した場合はSaveを止め、入力値を保持する
- dirty Codeは勝手にApplyしない
- dirty Codeがあれば PendingDraft(Code) として拒否する
- unresolved conflictがあれば「すべて保存済み」扱いにしない
- session-only stateがある場合は通常Saveで永続化できるか判定する
- external fingerprintを確認する

### Save中

CsvDocument::save() がexternal mergeを行った場合、その変更を DocumentChangeSet(origin=SaveSync) として返します。

そのChangeSetも通常のSession Reconcileへ通します。

save内部だけを再計算・invalidationsの例外にしません。

### Save後

全対象が成功し、Pending Draft / Unresolved Conflict / Not Persistable stateが残っていないときだけ「すべて保存済み」と表示します。

CSV file自体が最新diskと一致していてもLocal conflict draftが残っていれば、session全体は未解決です。

部分失敗は部分失敗として表示し、失敗したcategoryのdirty stateを維持します。

---

## GUI statusはstructured outcomeから作る

handlerが自由文字列で成功を宣言しません。

~~~text
Command
  ↓
SessionOutcome
  ↓
UI message
~~~

これにより「実際にはCodeが未適用なのに保存しました」のような偽成功を防ぎます。

---

## 実装レイヤー

~~~text
GUI / interactive shell
        ↓
InteractiveSession / Transition Coordinator
        ↓
Command / process
        ↓
CsvDocument
        ↓
data
~~~

InteractiveSession はGUI widgetを知りません。

保持するのはsession状態とtransitionだけです。

GUIは描画・入力・確認dialogに集中します。

DSL / LuauがInteractiveSession内で実行される場合も、実行前後のDocumentRevision / ChangeSetを通して同じReconcileを行います。

---

## 段階導入

### Phase 1: correctness

- DocumentRevision
- DocumentChangeSet
- Transition Guard
- Calculation effective freshness
- P0 merge / Code / Open bugs修正

不明な変更scopeは WholeTable として全再計算してよい。

### Phase 2: convergence

- external merge / Code merge core統一
- Save Coordinator
- structured SessionOutcome
- GUI handlerの個別dirty/recalculate/status処理を削減

### Phase 3: optimization

- exact changed-cell tracking
- selective calculation recalculation
- large CSV向けsnapshot / draft memory最適化

性能のためにPhase 1の安全性を弱めません。

---

## 今回のレビュー指摘との対応

| レビュー指摘 | 共通契約 |
| --- | --- |
| 同一cell競合でLocal消失 | shared 3-way merge / Local never lost |
| Code Applyで外部変更を巻き戻す | CodeDraft baseline + revision + 3-way merge |
| Openで未保存変更を破棄 | Transition Guard |
| Save sync後も古い計算結果 | DocumentChangeSet + effective freshness |
| ragged CSV writer不一致 | data I/O contract。別Issue #207 |
| XLSX長文字truncate | adapter lossless-or-error contract |
| rowlyx出力衝突 | backing/persistence preflight |
| 未適用CodeなのにSave成功表示 | Save Coordinator / structured outcome |
| MSRV不一致 | build contract。別Issue |

---

## 採用判断

今後、新しい状態をRowlyへ追加するときは次を答えられなければなりません。

1. これはCanonical / Draft / Rule / Derived / Backingのどれか。
2. 失われる可能性があるなら、Transition Guardにどう現れるか。
3. canonicalを変えるなら、どのChangeSetを出すか。
4. derived stateへどのinvalidationsを起こすか。
5. Save対象なら、SaveReportのどのcategoryか。
6. external changeと同時に起きた場合、Localをどう保持するか。

答えられない状態は、handlerへ直接追加しません。

この日本語版をRowlyのinteractive session状態遷移の正本仕様とします。