# GUI Command Registryの共有契約

## 背景と目的

menu、toolbar、shortcut、Command Palette、Alt KeyTipsが別々の操作定義を持つと、有効条件や実行処理が食い違います。Issue #109では、GUI操作の登録情報と実行直前の検査を一箇所へ集めます。Issue #113のKeyTipsも同じ登録情報を参照するため、専用shortcutとAlt経路を別のmetadataとして保持します。

共有契約の実装入口は `src/gui_commands.rs` です。`CommandRegistry` は画面やCSVを所有せず、GUI入口のmetadata・KeyTips・shortcut・availabilityを管理します。

**CommandRegistryをapplication commandの実装本体にはしません。**

GUI、DSL、CLI、Luauで意味が同じ操作は、最終的に共通のApplication Commandへ収束させます。

この共有契約だけでは既存GUIの操作は変わりません。menu・toolbarへの配線、Command Palette、Alt表示と階層操作は未接続です。KeyTips metadataの登録成功は、実GUIでAltから到達できることの証拠にはなりません。これらの接続と操作確認は #109 / #113で続けます。

## 登録情報

`CommandDefinition<Action>` は次の情報を保持します。

| 項目 | 意味 |
| --- | --- |
| `id` | 複数のGUI入口が共有するstable ID。登録後は変更しません。 |
| `name` / `description` / `category` | 表示・検索・ショートカット一覧に使う名前、説明、分類。 |
| `scope` | commandを提供するworkspace modeとfocusの組。 |
| `enabled` | CSVや履歴など、実行直前の動的状態を調べるpredicate。 |
| `shortcuts` | 専用shortcutと別名binding。専用shortcutなしの定義も許可します。 |
| `keytips_path` | Altから辿るlabelの階層。全定義で必須です。 |
| `action` | 検査に成功したときadapterへ渡す操作識別子。 |

`CommandContext` は現在のmode、focus、CSVの有無、CSVのUndo/Redo可否、未適用CSV textの有無を保持します。GUI adapterはcommandを呼ぶたびに現在の状態からcontextを作ってください。metadata一覧の表示時だけenabledを調べ、実行時には以前の結果を使う実装にしないでください。

`CommandScope` は許可するmode集合とfocus集合の直積です。動的predicateを呼ぶ前にscopeを調べます。dialog内の操作は `Modal`、text widgetの入力は `TextInput` として区別します。

## 登録時の検証

`CommandRegistry::new` は以下を拒否します。

- 空または空白を含むID、空の表示名・説明・分類、空のscope。
- 重複ID。scopeが違っても同じIDは登録できません。
- Alt経路の欠落、空label、英数字以外を含むlabel。
- 共通contextで同じ押下に一致するshortcut。論理Commandと物理Ctrl/Cmdの重なりもWindows/LinuxとmacOSの状態で検査します。
- 同一command内で重複するshortcut別名。
- 共通contextの同じAlt経路、同階層のprefix label、実行leafと子階層の兼用。

たとえば `F → O` と `F → S` は、ファイル階層を共有する別操作として登録できます。`F` そのものを実行leafにすると `F → O` と衝突します。同階層のlabel `F` と `FO` も、最初のFを押した時点で階層移動するか追加入力を待つか決められないため拒否します。大小文字の違いではAlt経路を区別しません。

modeまたはfocusが交わらない定義は、同じshortcutとAlt経路を再利用できます。動的enabled predicateの相互排他は、登録時の衝突回避の根拠にしません。

## GUI RegistryとApplication Commandを分ける

GUI Registryのstable IDは、可能な限りApplication Command IDと1対1に対応させます。

~~~text
GUI Button ─┐
Shortcut ───┤
Alt KeyTips ┤
Palette ────┘
      ↓
GUI Command Registry
      ↓
Application Command Request
      ↓
InteractiveSession / Coordinator
      ↓
process
~~~

DSL / CLI / Luauで同じ意味の操作を公開する場合は、GUI handlerを呼ぶのではなく同じApplication Commandへrequestを送ります。

### Application Command Request

概念:

~~~rust
struct CommandRequest {
    id: CommandId,
    arguments: CommandArguments,
    actor: CommandActor,
    interaction: InteractionMode,
    capabilities: CapabilityContext,
    state_stamp: Option<StateStamp>,
}
~~~

actor例:

~~~text
GUI
CLI
RowlyDsl
Luau
Internal
~~~

入口が違ってもCommand semanticsは変えません。

ただしsecurity capabilityとinteraction policyはactor/contextごとに異なってよいです。

### InteractionMode

~~~text
Interactive
NonInteractive
~~~

Interactiveでは、必要に応じてSave/Discard/Cancelやoverwrite確認等のInteractionRequestを返せます。

NonInteractiveではpromptを出しません。

確認が必要なら安定した `INTERACTION_REQUIRED` 相当errorを返します。

CLI `--no-input`、自動化、CIはNonInteractiveです。

### Preflight

commandはmutation前にpureなPreflightを行います。

~~~text
CommandRequest
  ↓
Preflight
  ├─ Ready(CommandPlan)
  ├─ Disabled(reason)
  ├─ NeedsInteraction(request)
  ├─ CapabilityDenied
  └─ InvalidArguments
~~~

UIのenabled表示と実dispatchは、同じCommandSpec / preflight contractから導出します。

ただしUIのenabled表示はhintにすぎません。
dispatch時に必ず再preflightします。

### State Stamp

dialogやfile chooserを跨ぐCommandPlanは、SESSION_STATE_TRANSITIONS.mdのState Stampを持ちます。

commit直前にstampが変わっていれば、古いplanを実行せず再preflightします。

### Command Outcome

GUIのstatus文字列をhandlerが直接成功状態として作りません。

概念:

~~~rust
struct CommandOutcome {
    command_id: CommandId,
    result: CommandResult,
    diagnostics: Vec<Diagnostic>,
}
~~~

CommandResult例:

~~~text
NoChange
Changed(SessionChangeSet)
Saved(SaveReport)
Transitioned(...)
Blocked(...)
~~~

GUIはOutcomeを表示へ変換します。
CLI `--json` は同じOutcomeを機械可読形式へ変換できます。

### Security

共通Application Commandだからといって全actorへ同じ権限を与えません。

~~~text
same command semantics
        +
actor-specific capability
        +
interaction policy
~~~

で実行可否を決めます。

Luau sandbox、DSL capability、CLI path scopeはこの境界で迂回できません。

## 共通dispatch

IDによるGUI入口は `dispatch` を使います。shortcutによる入口は `dispatch_shortcut` を使い、同じIDのdispatchへ合流します。無効なscope・状態の場合、Registryはhandlerを呼びません。handlerが操作失敗の `Result` を返した場合、Registryはその結果を変更せず返します。

`dispatch_shortcut` へは、eguiが正規化した押下modifierとlogical keyを渡してください。eguiの `matches_exact` を使い、Shift/Altの過不足を許可しません。たとえばCtrl+Shift+ZをCtrl+Zとして先に消費しません。一致する定義がなければ `None`、定義が見つかって動的に無効なら `Some(Err(DisabledCommand))` を返します。

以下は、gui featureを有効にしたRowlyを参照し、許可されたactionを取得する開発者向けのRust例です。サンプルはGUIやCSVの保存処理を呼びません。

```rust
use rowly::gui_commands::{
    CommandContext, CommandFocus, CommandMode, CommandRegistry,
    CoreCommand, core_command_definitions,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = CommandRegistry::new(core_command_definitions())?;
    let context = CommandContext {
        mode: CommandMode::TableEditor,
        focus: CommandFocus::Workspace,
        has_document: true,
        can_undo: false,
        can_redo: false,
        has_unapplied_csv_text: false,
    };
    let action = registry.dispatch("file.save", &context, |action| *action)?;
    assert_eq!(action, CoreCommand::Save);
    Ok(())
}
```

実GUIへ接続するときは、menu・toolbar・shortcut・KeyTipsから同じApplication Command Requestを作り、入口ごとの独立した操作実装を作らないでください。

GUI adapterはOpen/Save/Undo/Redoの意味を実装せず、引数収集とOutcome表示に集中します。

text widgetへfocusしている間のUndo/Redoは、CSV履歴commandへrequestを送らないようCommand scope / focusで区別します。

## 最初の基本定義

`core_command_definitions()` は以下の4定義を返します。Ctrl表記はWindows/Linuxのprimary modifierです。macOSではeguiの論理CommandによりCmdへ対応します。これはmodifier照合の契約であり、macOS GUIの実操作確認ではありません。

| ID | 専用shortcut | Alt経路metadata | 有効条件 |
| --- | --- | --- | --- |
| `file.open` | Ctrl+O | F → O | dialog外。 |
| `file.save` | Ctrl+S | F → S | dialog外、CSVあり、未適用CSV textなし。 |
| `edit.undo` | Ctrl+Z | E → U | Table EditorまたはViewerの通常focus、CSVあり、Undo可能。 |
| `edit.redo` | Ctrl+Y / Ctrl+Shift+Z | E → R | Table EditorまたはViewerの通常focus、CSVあり、Redo可能。 |

Import/Export、Calculation Binding、View grouping、Materializeなどの定義は、それぞれの操作をGUIへ接続する段階で追加します。未実装の操作を空handlerで登録して実行成功と扱わないでください。

## 確認方法と境界

`cargo test --all-features --test gui_command_registry` は、IDとshortcutの共通実行、無効context、未知ID、必須metadata、入口衝突、別名binding、platform modifier、handler失敗の返却をheadlessで確認します。`cargo fmt`、Clippy、buildと全テストも実行してください。

実GUIの到達性を確認するときは、全入口がRegistryを通ること、Altから実際に到達できること、dialog内のkeyboard操作が続くことを別途確認してください。この共有契約のテストだけで、GUI操作やKeyTips coverageの合格とは判定しません。
