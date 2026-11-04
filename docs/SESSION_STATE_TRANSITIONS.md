# セッション状態・変更適用・保存の設計契約

## 目的

RowlyではCSVが正本です。

しかし、実際の操作中にはCSV正本以外にも、未適用Code、セル編集中の値、Calculation Binding、derived result、Project設定、外部disk状態など複数の状態が同時に存在します。

これらをGUI handlerごとに個別同期すると、同じ操作でも入口によって挙動が変わり、保存漏れ・外部変更の上書き・古い計算結果表示が発生します。

この文書では、Rowlyの対話セッションが守る状態遷移の共通契約を定義します。

## 絶対条件

### 1. ローカル作業を黙って消さない

Rowly内で作られた未保存・未適用の作業は、明示的な破棄なしに消してはなりません。

対象には少なくとも次を含みます。

- canonical CSVの未保存編集
- Text Editorの未適用Code
- inline cell editの未確定値
- Calculation Bindingの未保存変更
- Project manifest / generated configのstaged変更
- .rowlyx workspaceの未pack変更
- merge conflictとして退避したLocal変更

外部変更、Open、source切替、Save、Close、Quitのどの経路でもこの原則を破ってはなりません。

### 2. 保存したと嘘をつかない

「保存しました」と表示するのは、保存対象として扱った状態が実際に保存済みになったときだけです。

未適用Codeを無視してCSVだけ保存した場合に「保存しました」と表示してはなりません。

複数の保存対象がある場合は、結果を分けて扱います。

~~~text
CSV: saved
Project config: failed
Package: not updated
~~~

この場合は全体成功ではありません。

### 3. `Evaluated` は現在の状態に対して正しいと確認できる場合だけ使う

Calculation resultの Evaluated は「以前計算に成功した」という意味ではありません。

**現在のcanonical CSVと現在のCalculation Bindingに対して正しいことを確認できる場合だけ Evaluated とします。**

古いdependency snapshotを持つresultを、handlerの再計算漏れだけで Evaluated 表示できてはなりません。

### 4. 正本CSVを変更したら必ず変更通知を返す

canonical CSVを変更したすべての操作は、変更内容を共通の DocumentChangeSet として上位層へ報告します。

変更元がGUIか、Codeか、外部同期か、Undoか、DSLか、Luauかで契約を変えません。

### 5. 開く・切り替える・閉じる・終了する操作は同じ遷移判定を使う

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
対話セッション
├─ 正本データ
├─ 未適用編集
├─ 規則・設定
├─ 派生状態
└─ 保存先・永続化
~~~

### 正本データ

物理CSVごとの CsvDocument が、現在確定済みのCSV表を保持します。

bare CSVでは通常1つ、Project / logical tableでは複数のphysical sourceが同時にcanonical dataへ参加できます。

~~~text
正本データ Context
├─ source A -> CsvDocument
├─ source B -> CsvDocument
└─ source C -> CsvDocument
       ↓
Logical Table / View
~~~

Edit / Code適用 / DSL / Luau / Undo / Redo / 外部同期等で変更されます。

ここへ入った変更は通常のhistory / validation / dirty contractに従います。

**logical tableはcanonical sourceを置き換える正本ではありません。**
表示rowにはprovenanceがあり、実際のwriteは元physical sourceへ帰ります。

### 未適用編集

まだcanonical CSVへ適用していないユーザー作業です。

~~~text
未適用編集
├─ InlineCellDraft
└─ CodeDraft
~~~

Draftは CsvDocument::is_dirty() と別物です。

Codeを編集しただけではcanonical CSVはdirtyではありませんが、**作業は失ってはいけません**。

したがって dirty == false を「捨ててもよい」と解釈してはいけません。

### 規則・設定

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

### 派生状態

canonical CSVとRulesから再生成できる状態です。

- Calculation result
- View interpretation
- 将来のindex / cache

派生状態は正本ではありません。

保存対象のdirtyとは区別し、Fresh / Stale / Pending / Error で管理します。

### 保存先・永続化

diskやproject packageなど、sessionの外側にある保存先です。

- CSV file + fingerprint
- .rwprj
- .rowlyx archive
- staged project workspace

---

## 未適用作業と未保存状態を分ける

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
    dirty_成果物s: BTreeSet<PersistenceArtifactId>,
}
~~~

dirty_成果物sには、たとえば次を個別に記録します。

~~~text
CsvSource(source_id / bare instance)
CalculationRules(source scope)
ProjectManifest
GeneratedConfig
PackageArchive
~~~

Project内でsource Aだけdirty、source Bはcleanという状態を一つのboolへ潰しません。

実際のTransition Guardでは、さらに次を独立categoryとして扱います。

~~~text
Session作業状態の要約
├─ Pending 未適用編集
├─ 未保存の永続化対象
├─ 未解決競合
└─ セッション限定・永続化不可
~~~

### 未解決競合

external mergeやCode適用で退避したLocal conflictは、canonical CSVがcleanでも未解決作業です。

conflict draftが残っている状態を「全部保存済み」と表示してはなりません。

### セッション限定・永続化不可

bare CSVのCalculation Bindingなど、現在のbackingへ保存できない状態です。

通常Saveで消えないため、Open / Close / Quit時はProject化・破棄・Cancel等の明示的なresolutionが必要です。

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

## 文書世代（`DocumentRevision`）

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

Revisionは内容のhashではありません。
Undo / Redoで過去と同じbytesへ戻ってもrevisionを巻き戻しません。

~~~text
A (rev 10)
→ B (rev 11)
→ Undo
→ A (rev 12)
~~~

これにより、一度別状態を経由したことを見失うABA問題を避けます。

### 識別子と世代を分離する

revision番号だけでは別documentを区別できないため、Draftや派生状態はrevision単体へ紐付けません。

概念:

~~~text
SessionId
  └─ DataContextId
       ├─ SourceInstanceId A
       │    ├─ DocumentRevision
       │    └─ BackingGeneration
       ├─ SourceInstanceId B
       │    ├─ DocumentRevision
       │    └─ BackingGeneration
       └─ WorkspaceRevision

RuleSet
  └─ RuleRevision

Derived
  └─ DerivedRevision
~~~

#### SessionId

InteractiveSessionそのもののidentityです。

Open / Switchで新しいdocumentへ入れ替えてもsession自体を維持する場合があります。

#### DataContextId

現在sessionで開いているbare CSVまたはProject data context全体のidentityです。

別Project / 別bare CSVへOpenした場合は新しいDataContextIdになります。

#### SourceInstanceId

physical CSV sourceごとのsession-local identityです。

- bare CSVを開き直せば新しいinstance
- project sourceはstable source_idとは別にsession-local SourceInstanceIdを持つ
- sourceを一度closeして再openした場合は新しいinstance
- Save Asだけなら同じsource instanceのbacking locator変更として扱える
- project内source moveはstable source_idを維持し、同じsource instanceのlocator変更として扱える

SourceInstanceIdはpathから生成しません。
pathはlocatorであり、rename / Save As / case / symlink解決で変わり得るためです。

session内の単調増加ID等、衝突しないopaque identityとして発行し、diskへ永続化しません。

Project sourceでは診断用にstable source_idも併記できますが、session object同一性はSourceInstanceIdで判定します。

CodeDraft / physical source用ConflictDraftは、対象SourceInstanceIdが一致しなければ利用しません。

#### WorkspaceRevision

active logical data context全体の世代です。

参加sourceのどれかがcanonical mutationした場合に進みます。

single-source sessionでもWorkspaceRevisionを持ってよいですが、Source DocumentRevisionと同じ値である必要はありません。

logical tableのView/cacheはWorkspaceRevisionで粗くinvalid化でき、source固有CalculationはSource DocumentRevisionで細かくfreshness判定できます。

#### BackingGeneration

disk fingerprintやproject/packageの保存世代です。

canonical content revisionとは別です。

改行・encoding・disk baselineだけが変わりcanonical tableが同じ場合、DocumentRevisionは増やさずBackingGenerationだけを更新できます。

#### RuleRevision

Calculation Binding / Validation / View rule等、CSV外ruleの世代です。

CSV内容が同じでもruleが変わればderived stateは更新対象です。

### 状態印（`StateStamp`）

長く開いたdialog、file chooser、将来のasync taskが古い前提でcommitしないよう、破壊的操作のplanはState Stampを持ちます。

概念:

~~~text
StateStamp
├─ DataContextId
├─ relevant SourceInstanceId(s)
├─ relevant DocumentRevision(s)
├─ WorkspaceRevision
├─ BackingGeneration(s)
└─ RuleRevision
~~~

plan作成時とcommit直前で必要なstampを比較し、不一致ならplanを捨てて再preflightします。

例:

1. Open確認dialogを表示
2. dialog表示中にexternal change hintが到着
3. 利用者がSaveして続行を選ぶ
4. commit前にdisk / session stampを再確認
5. 古い前提なら再syncしてresolutionを再評価

**確認dialogを一度出したから、その後の状態変化を無視してよい、とはしません。**

### Revision overflow

revisionはchecked incrementとし、overflow時はmutation前に失敗します。

mutation後にrevision更新だけ失敗する順序は禁止します。

---

## 状態の所有者を一箇所にする

同じ状態を複数箇所のboolで二重管理しません。

| 状態 | 所有者 | 派生表示 |
| --- | --- | --- |
| canonical CSV / history / disk baseline | CsvDocument | document dirty |
| Code draft / inline draft | InteractiveSession | pending work |
| Calculation rules / results | CalculationEngine | rule dirty / 実効結果 |
| project config staging / package state | ProjectSession | project dirty |
| selection / mode / popup | GUI | UI state |
| command availability | Session snapshotから導出 | enabled/disabled |
| status text | SessionOutcomeから導出 | message |
| session health | InteractiveSession | 正常 / 縮退 / 状態不明 |

例えば code_buffer_dirty と別の「Code未適用」flagを持ちません。
CodeDraftStatusを正本にし、UI表示はそこから導出します。

calculation_bindings_dirtyも、最終的にはRule/Project dirty stateの正本から導出し、GUI固有boolとして増殖させません。

---

## 全ての変更を直列化する

Rowlyのinteractive stateは単一書き込み主体とします。

バックグラウンド監視、将来のasync task、filesystem通知はInteractiveSessionを直接変更しません。

~~~text
background / watcher
       ↓
    イベント / 通知
       ↓
セッションイベントキュー
       ↓
遷移調停器
       ↓
同時に1つの変更だけ実行
~~~

これにより、Save途中にwatcher threadがdocumentを書き換えるようなreentrant mutationを禁止します。

filesystem watcherは従来どおり早期通知用hintです。
正しさは保存前のfingerprint / compare-and-swap検証で担保します。

---

## 変更通知（`DocumentChangeSet` / `SessionChangeSet`）

canonical mutationは共通のreceiptを返します。

概念:

~~~rust
struct DocumentChangeSet {
    source: SourceInstanceId,
    before_revision: DocumentRevision,
    after_revision: DocumentRevision,
    scope: ChangeScope,
    conflicts: Vec<MergeConflict>,
    history_effect: HistoryEffect,
}

struct SessionChangeSet {
    operation_id: OperationId,
    origin: ChangeOrigin,
    before_workspace_revision: WorkspaceRevision,
    after_workspace_revision: WorkspaceRevision,
    documents: Vec<DocumentChangeSet>,
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

disk fingerprint / encoding / baselineだけが変わりcanonical tableが変わらない場合はDocumentChangeSetを偽造しません。

その場合はBackingSyncReportとして別に報告します。

~~~text
BackingSyncReport
├─ source
├─ old backing generation
├─ new backing generation
└─ canonical_change: Option<DocumentChangeSet>
~~~

上位層は SessionChangeSet を受けてderived stateやDraftを更新します。

### 操作IDと変更グループ

1つのアプリケーションコマンドが複数physical sourceを変更しても、利用者から見れば1操作です。

そのためSessionChangeSetはsession-localなOperationIdを持ちます。

~~~text
row move command
  OperationId = 42
  ├─ source A: delete row
  └─ source B: insert row
~~~

両sourceのDocumentChangeSetを別のユーザー操作として扱いません。

OperationIdはpersistent history node IDではありません。
現在session内で変更を束ねるためのidentityです。

---

## アプリケーション層のUndoグループ

single-source編集は従来のCsvDocument historyで実装できます。

ただし複数ソース commandでは、application layerがOperationId単位のundo groupを管理します。

### 複数ソース Undo

row move等のUndoは:

~~~text
OperationId 42 Undo
  ├─ source Bからinserted rowを削除
  └─ source Aへremoved rowを復元
~~~

を一つのatomic application transactionとして行います。

片方だけUndoして途中状態を露出しません。

### process historyとの関係

各CsvDocument内部のhistoryはphysical sourceの編集証拠を保持できます。

application-level undo groupは、それらを跨ぐ調停を担当します。

将来の #70 persistent Git-like historyではOperationIdそのものを永続IDとして使う必要はありません。
persistent historyは別のimmutable node identityを持ち、session operation groupから履歴nodeを生成します。

---

## 変更適用パイプライン

canonical dataを変える操作は、入口に関係なく次を通ります。

~~~text
Command
  ↓
事前検査
  ↓
Canonical Mutation
  ↓
DocumentChangeSet
  ↓
セッション整合化
  ├─ Draft relation update
  ├─ Calculation invalidate / recalculate
  ├─ View/cache invalidate
  ├─ selection/bounds reconcile
  └─ status/result construction
~~~

GUI handlerが直接、document変更・calculation再計算・buffer更新・status設定を個別に並べてはいけません。

後処理を忘れる入口が必ず生まれるためです。

### 事前検査 / Plan / Commitを分ける

破壊的・複数stepの操作は、可能な限りmutation前にplanを完成させます。

~~~text
事前検査
  ↓
変更計画
  ↓
正本変更を確定
  ↓
変更結果 / 変更通知
  ↓
Reconcile
~~~

特に3-way mergeはpure functionとしてMergePlanを作り、競合判定が終わる前にcanonical tableを書き始めません。

validation、path collision、target存在確認、revision/capability条件など、mutation前に確認できるものは先に確認します。

### 失敗の分類

失敗を同じrollback規則で扱いません。

#### 変更前の失敗

例: validation error / parse error / conflict / path collision。

canonical stateを一切変更しません。

#### 正本変更中の失敗

一つの明示操作としてatomicに扱います。
途中成功を残しません。

複数physical sourceを変更するCommandも同じです。

例: logical table内のrow moveでsource Aから削除しsource Bへ追加する場合、

~~~text
preflight A + B
↓
複数ソース 変更計画
↓
A / Bを一つのapplication transactionとしてcommit
↓
SessionChangeSet(operation_id, documents = [A, B])
~~~

とし、Aだけ削除してBへの追加に失敗した状態を利用者へ露出しません。

commit中に後半sourceが失敗した場合は、前半sourceをtransaction内でrollbackしてからerrorを返します。
rollback自体に失敗した場合は通常errorへ丸めずセッション健全性を状態不明へ上げる設計を取ります。

#### 派生状態更新の失敗

canonical editは成功したままです。
CalculationはError/Staleへ落とし、canonical dataをrollbackしません。

#### 永続化の失敗

in-memory local editは維持します。
dirtyを消さず、SaveReportで失敗を返します。

#### 遷移準備の失敗

現在sessionを維持し、新sessionへswapしません。

### セッション健全性

通常は 正常 です。

~~~text
SessionHealth
├─ 正常
├─ 縮退
└─ 状態不明
~~~

#### 縮退

一部derived機能やwatcherが使えないが、canonical stateとbacking stateの関係は把握できている状態です。

例: watcher開始失敗。保存前fingerprint検証は可能。

#### 状態不明

rollback失敗、replace結果の確認不能等により、in-memoryとbackingの関係をRowlyが証明できない状態です。

状態不明では通常Saveや破壊的transitionを継続せず、再読込比較 / Recovery Copy / recovery手順を要求します。

**分からないのにClean/Savedと推測しません。**

### 状態不明Artifact

状態不明はsession全体の曖昧なboolではなく、影響成果物を記録します。

~~~text
状態不明Artifact
├─ 成果物 identity
├─ last known backing generation
├─ intended before state
├─ intended after state
├─ current in-memory state
└─ failure diagnostic
~~~

### 状態不明時に許可する操作

原則としてread-onlyとrecovery系だけです。

- current in-memory内容の閲覧
- backingの再読込・比較
- conflict / recovery diff表示
- Recovery Copyとして別pathへ保存
- 明示的なReload / Keep Disk
- 明示的なローカルを再適用 / merge
- Cancel / Quit resolution

通常のSave、Pack、Source Move、構造編集等は状態確定まで拒否します。

### Reconcile 状態不明

~~~text
1. affected backingを再読込
2. last known before / intended after / current diskを比較
3. 一意に状態を確定できればHealthを戻す
4. LocalとDiskが分岐していれば3-way merge / Conflictへ移行
5. どちらとも一致せず判断不能なら状態不明を維持
~~~

recoveryでLocalを採用する場合も、通常のcanonical mutation / history / ChangeSet契約を通します。

---

## セッション整合化

DocumentChangeSet を受けた後処理を一箇所へ集めます。

### Draft

CodeDraftがCleanならcanonical textへ同期してよい。

CodeDraftがDirtyならbufferを上書きしません。

base revisionとcurrent revisionが違えば Diverged にします。

### 計算結果

ChangeScope::Cells なら依存graphから影響するbindingだけを更新します。

Structure / WholeTable なら安全側へ倒し、全bindingを再評価またはstale化します。

### View / キャッシュ

canonical revisionに依存するcacheは無効化します。

### Selection

row/column削除等で選択位置が範囲外になった場合だけclampします。

---

## 計算結果は現在状態に対して自己検証する

event通知だけに正しさを依存させません。

現在の CalculationResult はdependency snapshotを保持しています。

表示・Materialize時には少なくとも次を検証します。

- resultのbinding revisionが現在のbinding revisionと一致
- dependency cellのraw valueがsnapshotと一致
- dependency先にbindingがある場合、そのbinding revisionが一致
- dependency先derived resultのrevisionが一致

一致しなければ保存済み状態が Evaluated でも、**effective statusはStale** とします。

概念:

~~~text
engine.result_for(document, id)
    -> FreshEvaluated(value)
    -> Stale(previous_value)
    -> Pending
    -> Error(...)
~~~

GUIは内部の CalculationResult.status を直接信用せず、current documentに対する実効結果を取得します。

Materializeも FreshEvaluated だけを受け付けます。

### Freshness判定順序

実効結果は次の順序で判定します。

1. DataContextId / SourceInstanceIdが一致する
2. bindingが存在し、binding revisionが一致する
3. targetが現在documentに存在する
4. evaluation時Source DocumentRevisionが現在revisionと同じならfast-pathでfresh
5. revisionが違う場合、dependency snapshotを現在値と比較する
6. upstream derived dependencyがある場合、そのeffective freshnessも確認する
7. すべて一致した場合だけfreshを維持する

unrelated cellだけが変わった場合はDocumentRevisionが進んでもdependency snapshotが同じなら、結果を再計算せずfreshと判定できます。

一方、dependencyが変わった場合は保存済み状態がEvaluatedでも実効結果はStaleです。

以前のErrorも同様に、dependencyが変わったら「現在も同じError」とは断定しません。古いErrorはstaleな診断として扱い、再評価対象にします。

これにより、handlerが再計算を忘れても古い値を正常結果として確定できません。

---

## 3-way merge判定を一つにする

外部同期とCode適用で別々のmerge logicを持ちません。

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

- 外部同期: Base=disk baseline / Local=Rowly canonical local / Remote=new disk
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

### 外部同期

非競合変更は自動mergeしてよい。

競合はDisk/currentを現在値にし、Localをconflict draftへ退避する既存方針を維持します。

### 競合退避の生存期間

ConflictDraftは一時messageではなく、未解決Local workです。

最低限次を保持します。

~~~text
ConflictDraft
├─ 所有者 DataContextId / SourceInstanceId
├─ origin
├─ Base snapshot / scope
├─ Local snapshot / changes
├─ Other snapshot / changes
├─ created DocumentRevision
└─ 解決状態
~~~

解決状態:

~~~text
未解決
ローカル再適用で解決
現在値維持で解決
明示破棄
再基準化が必要
~~~

#### 自動削除禁止

Save、external refresh、unrelated editを理由に未解決 draftを自動削除しません。

#### Reapply前の再検証

draft作成後にcurrent canonicalが変わっている場合、作成時Other snapshotをそのまま前提に再適用しません。

対象scopeのcurrent stateを比較し、安全に再適用できない場合は 再基準化が必要 として残します。

#### 解決操作

- ローカルを再適用: canonical mutation。history対象
- 現在値を維持: Localを使わないことを利用者が明示してresolve
- 破棄: 明示破棄
- 再基準化 / 手動解決: 将来拡張

「Saveしたから競合解決済み」は禁止します。

### 未適用編集の基準

CodeDraftとInlineCellDraftは、作業開始時のbaseを持ちます。

CodeDraft:

~~~text
所有者 SourceInstanceId
base DocumentRevision
base table snapshot
current text
draft status
~~~

InlineCellDraft:

~~~text
所有者 DataContextId
target CellAnchor
base WorkspaceRevision
base source DocumentRevision
base raw value
current edit value
draft status
~~~

### CellAnchor

selectionのdisplay row/columnをそのまま永続targetにしません。

概念:

~~~text
CellAnchor
├─ source identity / SourceInstanceId
├─ base physical coordinate
├─ column identity
├─ row provenance when available
└─ base raw value
~~~

logical table上ではdisplay row numberではなくprovenanceを優先します。

external structural change後に同じtargetを一意に再解決できない場合、古いdisplay coordinateへ書き込みません。

### Inline draft status

~~~text
Editing
Diverged
Conflict
Invalid
~~~

canonical CSVが別経路で変わってもdraft text/valueは上書きしません。

- targetと無関係な変更だけならEditingを継続できる
- target raw valueがBaseから変わったらConflict
- row/column構造変更後にtargetを一意に再解決できなければConflict
- validation違反ならInvalidとして入力値を保持

外部同期を理由に `editing=false` としてdraftを破棄する実装は禁止します。

### Commit

commit直前にCellAnchorをcurrent stateへ再解決します。

安全に同一targetと証明できた場合だけ通常のcell editとして適用します。

conflict時はcurrent canonicalを勝手に上書きせず、利用者のedit valueをDraftとして保持します。

### Code適用

Codeはユーザーが明示的にApplyする一つの編集単位です。

- base revisionとcurrent revisionが同じなら通常Apply
- divergedなら3-way merge
- conflictが1件でもあれば、Code由来の変更は**一件もcanonicalへ適用しない**
- Code bufferをそのまま保持する
- conflict情報を表示し、解決後に再Applyする
- 競合解決前に「Apply成功」と表示しない

外部同期は非競合部分を自動mergeしてよい一方、Code適用はatomicに扱います。

理由は、Apply Codeが利用者の明示した1つの編集transactionだからです。

部分適用して「どこまで入ったか」を別状態にするより、全件成功か全件未適用かを維持します。

将来、明示的な「非競合部分だけ適用」を追加する場合は別Commandとして扱います。

---

## 遷移判定

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
- 破棄
- Cancel

Open / Close / Quitごとに別の確認ロジックを作りません。

### 遷移判定は真偽値ではなく必要な解決操作を返す

未保存状態があるかだけでなく、**何をすればtransition可能になるか**を返します。

概念:

~~~text
TransitionGuardResult
├─ Ready
└─ Blocked
    ├─ 作業状態の要約
    └─ 解決操作
~~~

解決操作の例:

- SavePersistable
- CommitInlineEdit
- ReviewOrApplyCode
- ResolveConflict
- ConvertToProject
- 破棄SelectedWork
- 破棄AllLocalWork
- Cancel

CodeDraftやConflictは通常Saveだけでは解決しないため、単純なSave / 破棄 / Cancel三択へ押し込めません。

UIはこのresolution modelを表示するだけで、Open / Quitごとの独自判断を持ちません。

### 遷移は準備と確定を分ける

Open / source switchは、現在sessionを先に破棄しません。

~~~text
1. Guard
2. User resolution
3. 次のセッションを準備
4. 次のセッションを検証
5. セッション切替を確定
6. 旧セッションを破棄
~~~

新しいCSV / Projectのopen、Code text生成、Calculation Binding復元、watcher準備の途中で失敗した場合は、現在sessionをそのまま維持します。

「古いsessionを消してから新しいsessionをopenする」順序は禁止します。

### 遷移中のイベント

確認dialogやfile chooserを開いている間もwatcher eventは失われないようqueueします。

ただしbackground側からsessionへ直接適用しません。

transition commit直前にqueueされたeventとState Stampを確認し、必要ならresolutionをやり直します。

Cancel時はqueueを捨てず、通常sessionへ戻って処理します。

---

## 保存調停

Saveを単一のbool結果にしません。

### 「Saved」の定義

次の2つを区別します。

~~~text
保存先と一致
セッション解決済み
~~~

- 保存先と一致: 対象成果物へ現在のpersist対象stateが書き込まれている
- セッション解決済み: Pending Draft / Conflict / Not Persistable stateも残っていない

CSVがdiskと一致していてもCodeDraftやConflictが残っていれば保存先と一致ではあってもセッション解決済みではありません。

status barの単語「Saved」は曖昧なので、最終的にはこの区別を表示へ反映します。

概念:

~~~rust
struct SaveReport {
    成果物s: Vec<ArtifactSaveReport>,
    pending: Vec<PendingWork>,
    conflicts: Vec<未解決ConflictId>,
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
}
~~~

### 保存前

- active inline editがある場合は、通常のcell validationを通してcommitを試みる
- inline editのcommitに失敗した場合はSaveを止め、入力値を保持する
- dirty Codeは勝手にApplyしない
- dirty Codeがあれば PendingDraft(Code) として拒否する
- unresolved conflictがあれば「すべて保存済み」扱いにしない
- session-only stateがある場合は通常Saveで永続化できるか判定する
- external fingerprintを確認する

### 保存中

CsvDocument::save() がexternal mergeを行った場合、その変更を DocumentChangeSet(origin=SaveSync) として返します。

そのChangeSetも通常のセッション整合化へ通します。

save内部だけを再計算・invalidationsの例外にしません。

### 外部変更との競合

Saveは無制限に自動retryしません。

1. command開始時に最新diskを確認
2. 必要ならmerge / reconcile
3. expected fingerprint付きでwrite
4. commit直前に再度変わっていたらConcurrentModificationとして失敗

競合した場合は新しいdisk stateを次のsync対象として通知し、同じSave command内で無限にmerge/retryしません。

### 複数成果物の原子性

1.0で「atomic」と呼ぶ範囲を明確にします。

- CSV単体write: temporary + atomic replace
- .rowlyx final archive: temporary package + atomic replace
- .rwprj directory projectの複数CSV/config: **成果物単位ではatomicだが、複数成果物全体のクラッシュ時原子性は保証しない**
- logical tableの複数source Save: sourceごとにArtifactSaveReportを返し、全source成功を一つのboolへ潰さない

複数成果物 Saveで一部だけ成功した場合はSaveReportで必ず部分成功として示します。

将来multi-成果物 crash transactionを導入する場合も、CSVやproject形式を新しい正本へ置き換えず、ephemeral journal / stagingとして設計します。

### 保存後

全対象が成功し、Pending Draft / 未解決 Conflict / Not Persistable stateが残っていないときだけ「すべて保存済み」と表示します。

CSV file自体が最新diskと一致していてもLocal conflict draftが残っていれば、session全体は未解決です。

部分失敗は部分失敗として表示し、失敗したcategoryのdirty stateを維持します。

---

## GUIの状態表示は構造化された実行結果から作る

handlerが自由文字列で成功を宣言しません。

~~~text
Command
  ↓
SessionOutcome
  ↓
UI message
~~~

これにより「実際にはCodeが未適用なのに保存しました」のような偽成功を防ぎます。

### Command availabilityも同じ事前検査から導出する

menu / toolbar / shortcut / KeyTips / Command Paletteのenabled条件を別々に書きません。

~~~text
セッション状態スナップショット
    ↓
Command 事前検査
    ├─ Enabled / 無効理由
    └─ Dispatch時にも再検証
~~~

UIのdisabled表示はUX上のhintです。
正しさはdispatch時の同じpreflight再検証で保証します。

これにより「toolbarではSave disabledだがFile menuでは実行できる」のような入口差を構造的に禁止します。

---

## 実装レイヤー

~~~text
GUI / interactive shell
        ↓
InteractiveSession / 遷移調停器
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

### GUIとアプリケーションの境界

GUIは次の情報をapplicationへ渡してよいです。

- command id
- user input value
- selected target identity
- dialogで選ばれたresolution action

GUIが次を決定してはいけません。

- dirtyかどうか
- Save可能かどうか
- conflictが解決済みか
- calculation resultがfreshか
- transitionして安全か

これらはInteractiveSession / processの正本状態から導出します。

DSL / LuauがInteractiveSession内で実行される場合も、実行前後のDocumentRevision / ChangeSetを通して同じReconcileを行います。

---

## 段階導入

### 第1段階: 正しさを保証する

- DocumentRevision
- DocumentChangeSet
- Transition Guard
- Calculation effective freshness
- P0 merge / Code / Open bugs修正

不明な変更scopeは WholeTable として全再計算してよい。

### 第2段階: 共通経路へ収束させる

- external merge / Code merge core統一
- 保存調停
- structured SessionOutcome
- GUI handlerの個別dirty/recalculate/status処理を削減

### 第3段階: 最適化する

- exact changed-cell tracking
- selective calculation recalculation
- large CSV向けsnapshot / draft memory最適化

性能のためにPhase 1の安全性を弱めません。

---

## 保証の境界

この契約の「Local workを失わない」は、Rowlyが制御できる通常のcommand / transition / external sync / save failureに対する保証です。

process crash、OS crash、電源断から未保存memory stateを復元するCrash Recovery Storeは別設計とします。

bare CSVの隣へ勝手なrecovery sidecarを作ることはしません。

将来crash recoveryを導入する場合は、OSのapplication data領域等を使うlocal recovery storeとして、security / privacy / cleanup / encryption要件を別途固定します。

未実装のcrash recoveryを「Local work never disappears silently」の実装済み保証へ含めて説明してはいけません。

---

## 今回のレビュー指摘との対応

| レビュー指摘 | 共通契約 |
| --- | --- |
| 同一cell競合でLocal消失 | shared 3-way merge / ローカル変更を失わない |
| Code適用で外部変更を巻き戻す | Code未適用編集の基準 + revision + 3-way merge |
| Openで未保存変更を破棄 | Transition Guard |
| Save sync後も古い計算結果 | DocumentChangeSet + effective freshness |
| ragged CSV writer不一致 | data I/O contract。別Issue #207 |
| XLSX長文字truncate | adapter 無損失かエラー contract |
| rowlyx出力衝突 | backing/persistence preflight |
| 未適用CodeなのにSave成功表示 | 保存調停 / structured outcome |
| MSRV不一致 | ビルド契約。別Issue |

---

## 採用判断

今後、新しい状態をRowlyへ追加するときは次を答えられなければなりません。

1. これはCanonical / Draft / Rule / Derived / Backingのどれか。
2. 失われる可能性があるなら、Transition Guardにどう現れるか。
3. canonicalを変えるなら、どのChangeSetを出すか。
4. derived stateへどのinvalidationsを起こすか。
5. Save対象なら、SaveReportのどのcategoryか。
6. external changeと同時に起きた場合、Localをどう保持するか。
7. そのstateの所有者は誰か。別moduleにshadow flagを作っていないか。
8. identityはDataContextId / SourceInstanceIdまで含むか。revisionだけで別sourceを誤認しないか。
9. failure時のatomicity domainはどこか。
10. command availabilityとdispatchが同じpreflightを使うか。

答えられない状態は、handlerへ直接追加しません。

この日本語版をRowlyの対話セッション状態遷移の正本仕様とします。