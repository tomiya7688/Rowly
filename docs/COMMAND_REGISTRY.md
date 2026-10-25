# GUI Command Registryの共有契約

## 背景と目的

menu、toolbar、shortcut、Command Palette、Alt KeyTipsが別々の操作定義を持つと、有効条件や実行処理が食い違います。Issue #109では、GUI操作の登録情報と実行直前の検査を一箇所へ集めます。Issue #113のKeyTipsも同じ登録情報を参照するため、専用shortcutとAlt経路を別のmetadataとして保持します。

共有契約の実装入口は `src/gui_commands.rs` です。`CommandRegistry` は画面やCSVを所有せず、検査に成功したactionをadapterのhandlerへ渡します。実際のCSV操作はGUI adapterから既存のprocess APIへ接続してください。

既存GUIのOpen・Save・Undo・Redoは、menu・toolbar・専用shortcutから共有Registryを通して実行します。Table Editor・Text Editor・Viewerへの画面切替も、View menuとtoolbarから共有Registryを通して実行します。Text EditorのCSVテキスト適用も、Edit menu・toolbar・Altから共有Registryを通して実行します。ファイル・履歴・テキスト適用・画面切替のAlt表示と階層操作もRegistryを使います。Command Paletteと残るGUI操作のAlt経路は未接続です。KeyTips metadataの登録成功は、実GUIでAltから到達できることの証拠にはなりません。Palette、残るGUI操作のKeyTips接続、GUI実操作の確認は #109 / #113で続けます。

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

## 共通dispatch

IDによる入口は `dispatch` を使います。shortcutによる入口は `dispatch_shortcut` を使い、同じIDのdispatchへ合流します。無効なscope・状態の場合、Registryはhandlerを呼びません。handlerが操作失敗の `Result` を返した場合、Registryはその結果を変更せず返します。

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

実GUIへ接続するときは、同じhandlerからOpen/Save/Undo/Redoを呼び、menu・toolbar・shortcutごとの独立した操作実装を作らないでください。text widgetへfocusしている間のUndo/Redoは、CSV履歴へ渡さないでください。

## 既存GUIへの接続

`src/gui/command_dispatch.rs` は、GUI状態からcontextを作り、Registryが許可した操作を `open_csv`・`save_csv`・`undo`・`redo`・`apply_code_buffer`・`switch_workspace_mode` へ渡します。ファイル・履歴・テキスト適用・画面切替のmenu・toolbarは、Registryの表示名・説明・enabled条件を使います。画面のbuttonを押した後にも現在contextを再検査します。Registryの登録失敗は起動時のstatusへ表示し、Registryに登録した基本操作を提供しません。

focusはeguiのmodal layerを最優先します。inline cell編集中、またはfocus中のwidgetがTextEditの場合は `TextInput` とし、CSV履歴のUndo/Redoを無効にします。Text EditorではCSV履歴を呼ばず、CSV text widget自身のUndo/Redoを使います。HelpとViewerでもOpen/Saveの共通条件を使い、Viewerの通常focusではCSV履歴を実行できます。未適用CSV textがある場合、別modeへ移った後もSaveを無効にします。

shortcutはgridやtext widgetの描画前に、押下イベント自身のmodifierとlogical keyを使って照合します。登録定義とscopeが一致した押下だけを消費します。text入力へfocus中のUndo/Redoや未登録のkeyはwidgetへ残します。scopeが一致した候補は、有効状態にかかわらず消費します。実行時の最新状態で無効な場合はstatusへ拒否理由を表示します。長押しのrepeatイベントは消費しますが、同じ操作を繰り返しません。同frameの複数の押下も、操作ごとに最新contextで再検査します。たとえばUndoに続くRedoは、Undo後に利用可能になった履歴を参照します。

### 基本操作の実行方法

1. toolbarのpath欄へCSVまたはproject（`.rwprj` / `.rowlyx`）のpathを入力し、［ファイルを開く］またはCtrl+Oを実行してください。Rowlyは指定したCSV、またはproject内で最初に宣言されたfile sourceを開きます。Table EditorまたはViewerでは読み込んだ表を表示し、Text EditorではCSV textを表示します。Help表示中はHelpを維持します。読み込み失敗時は現在のdocumentを保持し、statusへ理由を表示します。
2. CSVへ適用済みの変更を保存するときは、［変更を保存］またはCtrl+Sを実行してください。CSV textの未適用変更がある場合は、Text Editorの［CSVテキストを適用］で変更を適用してください。Rowlyは未適用変更が残る間、保存を無効にします。project利用時は、CSVの保存成功後に計算規則とproject設定を保存します。設定保存の失敗時はstatusへ理由を表示しますが、先に成功したCSV保存は取り消しません。
3. Table EditorまたはViewerでCSV編集を取り消すときは、入力欄からfocusを外し、［CSV編集を元に戻す］またはCtrl+Zを実行してください。cellのinline編集中はEnterで入力を確定するか、Escapeで入力を取り消してください。再適用するときは［CSV編集をやり直す］、Ctrl+Y、Ctrl+Shift+Zのいずれかを実行してください。

menuのFileとEditにも同じ4操作があります。macOSの専用shortcutはCtrlの代わりにCmdを使います。実GUIの操作確認は別途必要です。

### CSVテキストの適用方法

Text EditorでCSVテキストを編集した後、toolbarまたはEdit menuの［CSVテキストを適用］を実行してください。Altを押して離し、E、Aの順に押しても適用できます。RowlyはCSVテキストを解析し、設定済みの検証規則を満たす場合だけCSV正本と履歴を更新します。CSVの解析または検証に失敗した場合、Rowlyは失敗理由をstatusへ表示し、CSV正本と履歴、修正途中のテキストを保持します。適用だけではdisk上のファイルを変更しません。ファイルへ保存するには、適用成功後に［変更を保存］を実行してください。

CSV未読込時、Text Editor以外の画面、modal dialog表示中は［CSVテキストを適用］を無効にします。専用shortcutは追加していません。

### 作業画面の切替方法

toolbarまたはView menuで［Table Editor］、［Text Editor］、［Viewer］を選択してください。Rowlyは指定した作業画面へ切り替えます。CSV未読込時も画面を切り替えられます。Help表示中はView menuまたはAlt経路から作業画面へ戻れます。modal dialog表示中は背後の画面切替を無効にします。

別画面からText Editorへ移る際、未適用のCSVテキスト変更がなければ、Rowlyは現在のCSV正本から編集用テキストを取得します。Text Editorの再選択では編集用テキストを再取得しません。未適用のCSVテキスト変更やセルのinline編集中の値は、画面切替だけでは適用・破棄しません。画面切替はCSV正本、disk上のファイル、Undo/Redo履歴を変更しません。未適用のCSVテキスト変更が残る間は、画面切替後も保存を無効にします。

## 登録済み基本操作のAlt階層

修飾キーを併用せずAltを押して離すと、menu直下に現在のmodeとfocusで選べるキーを表示します。Fを押すとファイル操作、Eを押すと編集操作、Vを押すと画面切替の候補へ進みます。ファイル階層ではOでOpen、SでSaveを実行してください。編集階層では、Table EditorまたはViewerの通常focusでUによるUndo、RによるRedoを実行してください。Text EditorではAでCSVテキストを適用してください。表示階層ではEでTable Editor、CでText Editor、VでViewerへ切り替えてください。経路と表示名はRegistryから取得し、無効な操作はdisabled表示します。leafを選んだ時も共通dispatchが最新contextを検査します。

Escは入力途中のlabelを消去するか、一階層戻ります。rootでEscを押すとKeyTipsを終了します。Altの再押下、windowのfocus喪失、modeまたはfocus中widgetの変更、mouse押下でもKeyTipsを終了します。Alt+Shiftなどの組合せでは開始しません。Shift併用またはIME入力に移った場合もKeyTipsを終了し、言語入力を保持します。階層選択に使った文字は入力欄へ配送しません。操作完了後に別のキーで入力した文字は保持します。

登録済み基本操作のヒントはmenu直下の一つのoverlayで表示します。各widgetの近くへの配置、残る全GUI操作、dialog内の経路、Alt→Kの一覧は未実装です。部分実装を #113 全体のAlt到達保証として扱わないでください。

## 最初の基本定義

`core_command_definitions()` は以下の8定義を返します。Ctrl表記はWindows/Linuxのprimary modifierです。macOSではeguiの論理CommandによりCmdへ対応します。これはmodifier照合の契約であり、macOS GUIの実操作確認ではありません。

| ID | 専用shortcut | Alt経路metadata | 有効条件 |
| --- | --- | --- | --- |
| `file.open` | Ctrl+O | F → O | dialog外。 |
| `file.save` | Ctrl+S | F → S | dialog外、CSVあり、未適用CSV textなし。 |
| `edit.undo` | Ctrl+Z | E → U | Table EditorまたはViewerの通常focus、CSVあり、Undo可能。 |
| `edit.redo` | Ctrl+Y / Ctrl+Shift+Z | E → R | Table EditorまたはViewerの通常focus、CSVあり、Redo可能。 |
| `edit.apply_csv_text` | なし | E → A | Text Editorのdialog外、CSVあり。 |
| `view.table_editor` | なし | V → E | dialog外。CSVの有無にかかわらず有効。 |
| `view.text_editor` | なし | V → C | dialog外。CSVの有無にかかわらず有効。 |
| `view.viewer` | なし | V → V | dialog外。CSVの有無にかかわらず有効。 |

Import/Export、Calculation Binding、View grouping、Materializeなどの定義は、それぞれの操作をGUIへ接続する段階で追加します。未実装の操作を空handlerで登録して実行成功と扱わないでください。

## 確認方法と境界

`cargo test --features gui gui::command_tests --lib` は、実CsvDocumentとeguiの入力frameで基本4操作のAlt経路、保存拒否、入力消費、長押し、focusとmodal、取消を確認します。OSのwindowを作らない検証であり、実GUI操作とは区別します。

`cargo test --features gui gui::workspace_command_tests --lib` は、全画面間のAlt切替、選択文字の消費、CSV正本・履歴・未適用編集の保持、CSV未読込時とHelpの復帰先、modal中の切替拒否をeguiの入力frameで確認します。画面切替のテストもOSのwindowを作らない検証です。

`cargo test --features gui gui::text_command_tests --lib` は、AltからのCSVテキスト適用、履歴と明示保存の分離、mode・focus・CSV有無による拒否、検証失敗時のCSV・履歴・未適用テキストの保持を確認します。テキスト適用のテストもOSのwindowを作らない検証です。

`cargo test --all-features --test gui_command_registry` は、IDとshortcutの共通実行、無効context、未知ID、必須metadata、入口衝突、別名binding、platform modifier、handler失敗の返却をheadlessで確認します。`cargo fmt`、Clippy、buildと全テストも実行してください。

実GUIの到達性を確認するときは、全入口がRegistryを通ること、Altから実際に到達できること、dialog内のkeyboard操作が続くことを別途確認してください。この共有契約のテストだけで、GUI操作やKeyTips coverageの合格とは判定しません。
