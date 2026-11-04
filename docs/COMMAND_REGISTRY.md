# GUI Command Registryの共有契約

## 背景と目的

menu、toolbar、ショートカット、Command Palette、Alt KeyTipsが別々の操作定義を持つと、有効条件や実行処理が食い違います。Issue #109では、GUI操作の登録情報と実行直前の検査を一箇所へ集めます。Issue #113のKeyTipsも同じ登録情報を参照するため、専用ショートカットとAlt経路を別の登録情報として保持します。

共有契約の実装入口は `src/gui_commands.rs` です。`CommandRegistry` は画面やCSVを所有せず、GUI入口の登録情報・KeyTips・ショートカット・有効可否を管理します。

**CommandRegistryをapplication commandの実装本体にはしません。**

GUI、DSL、CLI、Luauで意味が同じ操作は、最終的に共通のアプリケーションコマンドへ収束させます。

この共有契約だけでは既存GUIの操作は変わりません。menu・toolbarへの配線、Command Palette、Alt表示と階層操作は未接続です。KeyTips 登録情報の登録成功は、実GUIでAltから到達できることの証拠にはなりません。これらの接続と操作確認は #109 / #113で続けます。

## 登録情報

`CommandDefinition<Action>` は次の情報を保持します。

| 項目 | 意味 |
| --- | --- |
| `id` | 複数のGUI入口が共有する安定ID。登録後は変更しません。 |
| `name` / `description` / `category` | 表示・検索・ショートカット一覧に使う名前、説明、分類。 |
| `scope` | commandを提供するworkspace modeとfocusの組。 |
| `enabled` | CSVや履歴など、実行直前の動的状態を調べる判定条件。 |
| `shortcuts` | 専用ショートカットと別名binding。専用ショートカットなしの定義も許可します。 |
| `keytips_path` | Altから辿るlabelの階層。全定義で必須です。 |
| `action` | 検査に成功したときadapterへ渡す操作識別子。 |

`CommandContext` は現在のmode、focus、CSVの有無、CSVのUndo/Redo可否、未適用CSV textの有無を保持します。GUIアダプタはcommandを呼ぶたびに現在の状態からcontextを作ってください。登録情報一覧の表示時だけenabledを調べ、実行時には以前の結果を使う実装にしないでください。

`CommandScope` は許可するmode集合とfocus集合の直積です。動的判定条件を呼ぶ前に適用範囲を調べます。dialog内の操作は `Modal`、text widgetの入力は `TextInput` として区別します。

## 登録時の検証

`CommandRegistry::new` は以下を拒否します。

- 空または空白を含むID、空の表示名・説明・分類、空の適用範囲。
- 重複ID。適用範囲が違っても同じIDは登録できません。
- Alt経路の欠落、空label、英数字以外を含むlabel。
- 共通contextで同じ押下に一致するショートカット。論理Commandと物理Ctrl/Cmdの重なりもWindows/LinuxとmacOSの状態で検査します。
- 同一command内で重複するショートカット別名。
- 共通contextの同じAlt経路、同階層のprefix label、実行leafと子階層の兼用。

たとえば `F → O` と `F → S` は、ファイル階層を共有する別操作として登録できます。`F` そのものを実行leafにすると `F → O` と衝突します。同階層のlabel `F` と `FO` も、最初のFを押した時点で階層移動するか追加入力を待つか決められないため拒否します。大小文字の違いではAlt経路を区別しません。

modeまたはfocusが交わらない定義は、同じショートカットとAlt経路を再利用できます。動的enabled 判定条件の相互排他は、登録時の衝突回避の根拠にしません。

## GUIコマンド登録とアプリケーションコマンドを分ける

GUIコマンド登録の安定IDは、可能な限りアプリケーションコマンド IDと1対1に対応させます。

~~~text
GUI Button ─┐
Shortcut ───┤
Alt KeyTips ┤
Palette ────┘
      ↓
GUI Command Registry
      ↓
アプリケーションコマンド要求
      ↓
InteractiveSession / Coordinator
      ↓
process
~~~

DSL / CLI / Luauで同じ意味の操作を公開する場合は、GUI処理部を呼ぶのではなく同じアプリケーションコマンドへ要求を送ります。

### アプリケーションコマンド要求

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

入口が違ってもコマンドの意味は変えません。

ただしセキュリティ権限と対話方針は実行主体 / 文脈ごとに異なってよいです。

### 対話方式

~~~text
対話あり
対話なし
~~~

対話ありでは、必要に応じてSave/Discard/Cancelやoverwrite確認等のInteractionRequestを返せます。

対話なしでは確認入力を求めません。

確認が必要なら安定した `INTERACTION_REQUIRED` 相当errorを返します。

CLI `--no-input`、自動化、CIは対話なしです。

### 事前検査

commandはmutation前にpureな事前検査を行います。

~~~text
CommandRequest
  ↓
事前検査
  ├─ 実行可能(CommandPlan)
  ├─ 無効(reason)
  ├─ 利用者操作が必要(request)
  ├─ 権限不足
  └─ 引数不正
~~~

UIのenabled表示と実dispatchは、同じCommandSpec / 事前検査の契約から導出します。

ただしUIのenabled表示は表示上の補助にすぎません。
dispatch時に必ず再preflightします。

### 状態印

dialogやfile chooserを跨ぐCommandPlanは、SESSION_STATE_TRANSITIONS.mdの状態印を持ちます。

commit直前にstampが変わっていれば、古いplanを実行せず再preflightします。

### コマンド実行結果

GUIのstatus文字列を処理部が直接成功状態として作りません。

概念:

~~~rust
struct CommandOutcome {
    command_id: CommandId,
    result: CommandResult,
    diagnostics: Vec<Diagnostic>,
}
~~~

コマンド結果例:

~~~text
変更なし
変更あり(SessionChangeSet)
保存結果(SaveReport)
遷移完了(...)
実行停止(...)
~~~

GUIは実行結果を表示へ変換します。
CLI `--json` は同じOutcomeを機械可読形式へ変換できます。

### セキュリティ

共通アプリケーションコマンドだからといって全actorへ同じ権限を与えません。

~~~text
同じ操作意味
        +
実行主体ごとの権限
        +
対話方針
~~~

で実行可否を決めます。

Luau sandbox、DSL capability、CLI path 適用範囲はこの境界で迂回できません。

## 共通dispatch

IDによるGUI入口は `dispatch` を使います。ショートカットによる入口は `dispatch_shortcut` を使い、同じIDのdispatchへ合流します。無効な適用範囲・状態の場合、Registryは処理部を呼びません。処理部が操作失敗の `Result` を返した場合、Registryはその結果を変更せず返します。

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

実GUIへ接続するときは、menu・toolbar・ショートカット・KeyTipsから同じアプリケーションコマンド要求を作り、入口ごとの独立した操作実装を作らないでください。

GUIアダプタはOpen/Save/Undo/Redoの意味を実装せず、引数収集とOutcome表示に集中します。

text widgetへfocusしている間のUndo/Redoは、CSV履歴commandへrequestを送らないようCommandScope / focusで区別します。

## 最初の基本定義

`core_command_definitions()` は以下の4定義を返します。Ctrl表記はWindows/Linuxのprimary modifierです。macOSではeguiの論理CommandによりCmdへ対応します。これはmodifier照合の契約であり、macOS GUIの実操作確認ではありません。

| ID | 専用ショートカット | Alt経路登録情報 | 有効条件 |
| --- | --- | --- | --- |
| `file.open` | Ctrl+O | F → O | dialog外。 |
| `file.save` | Ctrl+S | F → S | dialog外、CSVあり、未適用CSV textなし。 |
| `edit.undo` | Ctrl+Z | E → U | Table EditorまたはViewerの通常focus、CSVあり、Undo可能。 |
| `edit.redo` | Ctrl+Y / Ctrl+Shift+Z | E → R | Table EditorまたはViewerの通常focus、CSVあり、Redo可能。 |

Import/Export、Calculation Binding、View grouping、Materializeなどの定義は、それぞれの操作をGUIへ接続する段階で追加します。未実装の操作を空処理部で登録して実行成功と扱わないでください。

## 確認方法と境界

`cargo test --all-features --test gui_command_registry` は、IDとショートカットの共通実行、無効context、未知ID、必須登録情報、入口衝突、別名binding、platform modifier、処理部失敗の返却を画面なしで確認します。`cargo fmt`、Clippy、buildと全テストも実行してください。

実GUIの到達性を確認するときは、全入口がRegistryを通ること、Altから実際に到達できること、dialog内のkeyboard操作が続くことを別途確認してください。この共有契約のテストだけで、GUI操作やKeyTips coverageの合格とは判定しません。
