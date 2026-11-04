# GUIの土台の設計契約

View/Edit/Codeの3モード、eguiのCSV Spreadsheet Grid、有限範囲の選択とキーボード操作、Alt KeyTips、ショートカット一覧、Alt到達性の自動検証、GUI日本語フォントを一つの土台として定義します。

- 状態: 提案: Issue本文に基づく設計。利用者未承認
- 確認日: 2026-11-05
- 対象Issue: #6 View・Edit・Codeの3モードを実装する(親)、#57 eguiでCSV Spreadsheet Gridを実装する、#110 Spreadsheet選択をCSVの有限データ範囲に基づくキーボード操作として実装する、#112 キーボードショートカット一覧(Alt→K)、#113 AltキーでExcel風KeyTipsを表示する、#200 全GUI CommandのAlt到達性を自動検証する、#201 Alt到達性テストをWindows/Ubuntu CIの必須ゲートに追加する、#109 GUI操作をCommand Registryへ登録する、#230 GUIの日本語が四角く表示される問題を修正する
- 関連Issue: #199(Excelの操作経路の調査)、#263(View列layoutのRegistry/Alt接続)
- 読む人: GUI層(`src/gui.rs`、`src/gui/`、`src/gui_commands/`、`src/gui_help.rs`)を実装する開発者

## 背景と目的

現在のGUIは、3モードの責務、選択の境界、キーボード経路が文書として固定されていません。そのため次の問題が起きます。

- Edit・View・Codeのどこまでが編集で、どこからが表示だけかが画面ごとに揺れ、表示用の値をCSVへ書き込む実装が紛れ込むおそれがあります。
- 選択とキーボード移動の境界が決まっておらず、Excelの仮想的な無限シートのように存在しないセルを選べる実装になるおそれがあります。
- menu・toolbar・ショートカットが別々に操作を持つため、Altから辿れない操作が生まれます。
- 日本語が四角で表示され、CSVの内容もヘルプも読めません(#230)。

この文書は、CSVのraw cell textを唯一の正本に保ったまま、全GUI操作をCommand Registry経由にしてAltから到達可能にし、日本語を表示できるGUIの土台を定めます。Registryの登録情報と検証規則は[Command Registry](COMMAND_REGISTRY.md)、Value BarとCalculation Barは[Edit入力バー](EDIT_INPUT_BAR.md)、tabによる対象切替は[ワークスペースナビゲーション](WORKSPACE_NAVIGATION.md)、Codeモードの内部は[Codeモード編集器](CODE_EDITOR.md)、View設定は[View model](VIEW_MODEL.md)を正本とし、ここでは複製しません。

## 現行実装との差

確認したファイルは `src/gui.rs`、`src/gui/table_editor.rs`、`src/gui/document_editor.rs`、`src/gui/command_bar.rs`、`src/gui_commands/`(`core.rs`、`context.rs`、`registry.rs`)、`src/gui_main.rs`、`Cargo.toml` です。

| 領域 | 実装済み | 未実装 |
| --- | --- | --- |
| 3モード | `WorkspaceMode`(`TableEditor`・`TextEditor`・`Viewer`・`Help`)と `show_workspace` による描画切替。 | 3モードをキーボードCommandで切り替える入口、Edit/Code間の適用規則の接続。 |
| Grid | `show_editor_grid`(`table_editor.rs`)による表示、A1表記の `column_label`、inline編集の `begin_edit`、行列の `insert_row`・`delete_row`・`insert_column`・`delete_column`、`undo`・`redo`、`copy_selected_csv_values`。 | 固定ヘッダーを含むスクロール規約の文書化、行数が多い表での描画範囲の限定(可視行だけ生成)の確認。 |
| 選択 | `RowlyApp::selection`・`selection_anchor`(単一矩形)、`set_selection` と `update_selection` が `row_count`・`column_count` へ制限、`handle_grid_keys` の矢印キー・Shift+矢印。 | Tab/Enter移動、Ctrl+矢印のdata edge移動、行全体・列全体の選択、複数range、logical tableのprovenance付きrange。 |
| Command Registry | `CommandRegistry`、`core_command_definitions()`(`file.open`・`file.save`・`edit.undo`・`edit.redo`)、`dispatch`、`dispatch_shortcut`。 | GUIはRegistryを参照していません。`command_bar.rs` のmenuと `handle_grid_keys`・`handle_calculation_shortcuts` が個別に直接処理を呼びます。`calculation.*` 4件(`CalculationCommand`)もRegistry未登録です。 |
| Alt KeyTips | `CommandDefinition::keytips_path` の登録時検証のみ。 | Altの検出、KeyTipsの重ね描き、階層移動、Esc/Alt終了、`Alt → K`。 |
| ショートカット一覧 | なし。 | Registryから生成する一覧画面。 |
| 到達性の検証 | `tests/gui_command_registry.rs` が登録時の衝突を確認。 | 実GUI由来のCommandを網羅する検証と、CI専用step。`docs/CI.md` に方針はありますが、`.github/workflows/ci.yml` に該当stepはありません。 |
| 日本語フォント | なし。`Cargo.toml` の `eframe` は `default_fonts` のみで、`src/` にフォント登録処理はありません。 | 日本語フォントの同梱と登録(#230)。 |

menuの表示名は現状 `File`・`Edit`・`Formulas`・`View`・`Help` の英語です。日本語名への変更は、フォント修正(#230)の後に#113で行います。

## 設計方針

### 契約

1. CSVのraw cell textだけが正本です。選択、列ヘッダー、行番号gutter、KeyTipsのlabel、Viewの表示結果をCSVへ保存しません([データモデルの原則](DATA_MODEL_PRINCIPLES.md))。
2. GUIの全操作は、Command Registryに登録した安定IDを経由して実行します。menu・toolbar・ショートカット・KeyTips・ショートカット一覧は、同じ登録情報から作ります。入口ごとに別の操作実装を持ちません。
3. GUIから実行できる全Commandに、Alt KeyTipsのpathを必ず持たせます。ショートカットやCommand Paletteがあることは、path不要の理由にしません。
4. データを変更する操作は、既存のprocess境界(`CsvDocument` のcommandとその履歴)を通します。GUI専用の編集経路を作りません。
5. 選択と移動の境界は、現在のdocumentが持つ有限のrecord数とcolumn数です。存在しないセルを仮想的に作りません。

### 3モードの責務

| 利用者が見る名前 | 現行のenum | 責務 | CSV正本への書込み |
| --- | --- | --- | --- |
| Edit | `WorkspaceMode::TableEditor` | 表として編集し、式・規則・割当を編集する。 | Command経由でだけ書く。 |
| View | `WorkspaceMode::Viewer` | 式や規則を評価した答えを読む。 | 書かない。 |
| Code | `WorkspaceMode::TextEditor` | CSVの生テキストを見て直接編集する。 | 適用に成功したときだけ書く。 |

`WorkspaceMode::Help` はヘルプ表示であり、3モードには含めません。

Editで式を変更しても、その時点でCSVのraw valueは変わりません。計算結果をCSVへ書く操作は、明示的なMaterialize/Apply Result Commandだけです。ViewはEditの確定済み状態を読み、Codeに式やViewの表示結果を混ぜません。Code buffer内の未適用textは正本ではなく、Edit・Viewは確定済みCSVだけを読みます。Draftの扱いは[Codeモード編集器](CODE_EDITOR.md)、Viewの設定は[View model](VIEW_MODEL.md)に従います。

モード切替のCommand ID案は `mode.edit`・`mode.view`・`mode.code` です。IDは登録後に変更しません。Code bufferに未適用textがある間のモード切替は、Transition Guard([セッション状態遷移](SESSION_STATE_TRANSITIONS.md))に従い、黙って捨てません。

### Spreadsheet Grid(#57)

Gridは `RowlyApp` が保持する `selection` と `CsvDocument` を描画する層です。`A1` は常にCSVの1行1列目を指します。CSVの先頭行をViewで見出し表示しても、A1の対応は変わりません。列ヘッダー、行番号gutter、title bandはCSV外のUIとして描画し、保存対象にも座標にも含めません。

行数が多い表では、eguiの `ScrollArea::show_rows` など可視範囲だけを生成する方式を使い、全セルのwidgetを作りません。入力規則付きセルの候補editorは#106が担当し、Gridは編集拡張点を提供して、候補確定も通常のCommand経路で書き込みます。

### 選択モデル(#110)

データモデル案です。

```rust
enum SelectionShape {
    Cell,                        // active cellだけ
    Range { anchor: CellRef },   // anchorからactiveまでの矩形
    Rows { first: usize, last: usize },
    Columns { first: usize, last: usize },
}
struct SelectionSnapshot {
    active: CellRef,
    shape: SelectionShape,
    extra: Vec<SelectionShape>,  // 将来の複数range。1.0では空
}
```

現行の `selection` と `selection_anchor` は、`Range` に相当します。`SelectionSnapshot` は単一矩形へ固定せず、`extra` で複数rangeへ拡張できる形にします。logical tableを選ぶ場合は、各セルのprovenance(元のsourceと行)を保持します。

境界は `document.row_count()` と `document.column_count()` です。ragged row(列数の異なる行)は、`cell()` が `None` を返す位置を空欄とは区別して扱い、移動と選択は境界を越えません。0行のdocumentではactive cellを持たず、移動・選択のCommandは何もしません。

キーボード操作は次のとおりです。

| 操作 | キー | 動作 |
| --- | --- | --- |
| 移動 | 矢印、Tab/Shift+Tab、Enter/Shift+Enter | active cellを境界内で動かす。 |
| 範囲拡張 | Shift+矢印 | anchorを保ち矩形を広げる。 |
| data edge | Ctrl+矢印、Ctrl+Shift+矢印 | 連続した実データの端、なければ表の端へ移動または拡張する。 |
| 行・列選択 | Shift+Space、Ctrl+Space | 行全体・列全体を選ぶ。Escまたは矢印で通常の選択へ戻る。 |

行・列の選択はCSVへ何も書き込みません。新規row・columnは、選択移動では作らず、明示的な追加Commandか限定されたnew-entry位置だけから作ります。長時間のdialogにまたがる操作は、選択ではなく解決済みtargetとStateStampを使い、後から選択が変わっても別セルへ適用しません([Command Registry](COMMAND_REGISTRY.md)の「状態印」)。

採用しなかった案は、Excelと同じ仮想的な無限シートです。巨大な空白を暗黙に選択対象にすると、全選択や貼り付けが誤って大量のrecordを作るためです。

### Alt KeyTips(#113)と全Commandの到達性(#109)

KeyTips modeは、Altの単独押下で開始します。rootでは、現在のmodeとfocusで有効なCommandのKeyTips pathの先頭labelを重ね描きします。labelを押すと次の階層を表示し、最後のlabelでCommandをdispatchします。Escは1階層戻り、rootではmode終了です。Altの再押下でも終了します。実行後、またはmouse clickで別focusへ移った場合も終了します。表示するlabelは `CommandRegistry::commands()` から作り、menu側に別のlabel表を持ちません。

`Alt → K` は同時押しではなく、KeyTips modeで `K` を押すsequenceです。`K` は「キーボードショートカット」のtop-level項目で、一覧画面を開くCommand(ID案 `help.shortcuts`)に割り当てます。top-levelの頭文字は、`F`(ファイル)、`E`(編集)の既存登録に合わせ、衝突は登録時検証に任せます。

Command Palette(#109)は、同じRegistryの検索・実行入口です。Paletteとショートカット一覧はどちらも、Registryの `name`・`description`・`category`・`shortcuts` を読む画面であり、別の一覧表を持ちません。

### ショートカット一覧(#112)

一覧画面は、Registryの全定義を読んで次の列を表示します。操作名、現在のショートカット(未割当ならその旨)、分類、有効なmodeとfocus。上下キーとPageUp/PageDownで移動し、検索欄はCommand名とショートカット文字列で絞り込みます。Escで閉じます。手書きの一覧は作りません。

### 到達性の検証(#200、#201)

検証は、実GUIが使う `CommandRegistry` を入力にします。各Commandについて、command_id、有効な文脈、KeyTips path、path衝突、到達可能性を検査します。検査項目は、path欠落、同階層のlabel衝突、途中nodeの到達不能、有効な文脈からの到達不能、dialog内の主要actionが `Modal` focusで完了できること、の5点です。ショートカットの有無とPaletteの有無は、合否に使いません。

未登録のGUI操作を見逃さないため、menuやtoolbarの項目は、Registryのidを引数に受け取る共通関数だけで生成します。idを持たない手書きのボタンは、検証のfixtureでも失敗として扱います。専用のtest targetを `tests/` に置き、CIの独立stepから直接呼びます。方針は[CI](CI.md)の「Alt KeyTips 全到達性ゲート」に既に記載されているため、ここでは再掲しません。

### 日本語フォント(#230)

eframeの既定フォントは日本語の字形を持たないため、四角で表示されます。ライセンスを確認した日本語フォントをバイナリに同梱し、起動時に `egui::FontDefinitions` へ登録します。`Proportional` と `Monospace` の両方で、既定フォントの後ろにフォールバックとして追加します。

登録は `gui_main.rs` の `run_native` が渡す生成closureの中で、`cc.egui_ctx.set_fonts` を呼ぶ一箇所に集約します。OSのフォントには依存しません。フォントのライセンス本文と出典は、[ライセンス方針](LICENSING.md)の第三者コンポーネントとして配布物のNOTICEへ含めます。

## 利用者から見た操作

| 入口 | 操作 | Command ID案 |
| --- | --- | --- |
| GUI | Alt → V → E / V / C(要確認)でEdit/View/Codeを切り替える。 | `mode.edit`、`mode.view`、`mode.code` |
| GUI | Alt → K でショートカット一覧を開く。 | `help.shortcuts` |
| GUI | Ctrl+Shift+P(案)でCommand Paletteを開く。 | `palette.open` |
| GUI | Gridで矢印・Tab・Enter・Ctrl+矢印・Shift+Space・Ctrl+Spaceを押す。 | `selection.move`、`selection.extend`、`selection.rows`、`selection.columns` |

CLI、DSL、Luauには、このGUIの土台に固有の入口はありません。選択とモード切替はGUIの表示状態であり、CSVを変更する公開Commandではないためです。データを変更する操作は、[Command Registry](COMMAND_REGISTRY.md)のアプリケーションコマンドを通して他の入口と共有します。

失敗時の見え方は次のとおりです。無効なCommandは、KeyTipsとPaletteでdisabledとして区別して表示し、実行しません。Code bufferに未適用textがあるままモードを切り替えると、適用・破棄・取消のどれかを選ぶ確認を出します。未登録のKeyTipsキーを押した場合は、何も実行せず現在の階層に留まり、statusに「このキーに割り当てられた操作はありません」と表示します。

## 永続化と互換性

- CSV本文は、この設計のどの操作でも変更しません。選択、KeyTips、一覧、フォント登録はCSVに保存しません。
- 現在のモードと選択は、セッション内のUI状態です。`.rwprj` への保存は、このIssue群の対象外とし、必要な場合は[ワークスペースナビゲーション](WORKSPACE_NAVIGATION.md)の永続化規則に従います。
- ショートカットのカスタマイズは非対象です。KeyTipsのlabelは、Registryのコード上の登録情報が正本です。
- 同梱フォントのファイルとライセンスは、配布物に含めます。CSVやプロジェクト形式の互換性には影響しません。

## 安全性と失敗時の挙動

- モード切替と選択変更は、CSVを書かないため、失敗しても部分更新になりません。
- Code bufferの未適用textは、モード切替・外部変更・選択変更で黙って消しません。適用に失敗した場合、モードは切り替わらず、診断を表示します。
- Registryのdispatchは、実行の直前に現在の状態から文脈を作り直して有効性を再検査します。KeyTipsが開いている間にdocumentが閉じられた場合は、実行せずにmodeを終了します。
- 選択の座標は、document変更(行削除など)の後に、`clamp_selection` と同様に境界へ収め直します。範囲外のセルへ書き込みません。
- フォントの登録に失敗した場合でも、GUIは起動して英数字を表示し、statusに失敗を表示します。同梱フォントは埋め込みなので、通常は読み込みに失敗しません。

## 実装順序と依存

| 順 | Issue | 最小単位 | 先行条件 |
| --- | --- | --- | --- |
| 1 | #230 | 日本語フォントの同梱と登録、グリフ確認テスト。 | なし(他と並行可)。 |
| 1 | #109 | GUI入口(menu・ショートカット)をRegistryへ接続し、`calculation.*` を登録する。 | なし(並行可)。 |
| 2 | #57 | Gridの監査と、残る表示・スクロール規約の整備。 | なし(#110と編集範囲を分ける)。 |
| 3 | #110 | `SelectionSnapshot` とキーボード操作。 | #57。 |
| 3 | #113 | KeyTips mode、描画、階層操作、全GUI操作のpath付与。 | #109。 |
| 4 | #112 | Registry由来のショートカット一覧と `Alt → K`。 | #109、#113。 |
| 4 | #200 | 到達性のvalidatorと専用test target。 | #109、#113。 |
| 5 | #201 | CIの独立stepと集約checkへの接続。 | #200。 |
| 5 | #6 | 3モードの責務とモード切替Commandの統合確認。 | #57、#110と、Edit/Code個別Issue。 |

#230・#109・#57は互いに独立で、並行して進められます。#263のようにViewの機能を追加するIssueは、同じ到達性の検証の対象になるため、#200より後に追加します。

## 受け入れ条件と検証

- #57: A1がCSV先頭を指すこと、編集がprocess経由であること、1操作をUndo/Redoできること、gutterがCSVへ混入しないこと。単体テストで確認します。
- #110: 矢印・Shift・Tab・Enter・Ctrl+矢印・行列選択が有限範囲を越えないこと。0行、ragged row、空セルを含むCSVで境界を検証する単体テストを用意します。選択がCSVを書き換えないことも確認します。
- #113: Altで表示、階層、Esc・Alt終了を単体テストと実GUI操作で確認します。
- #112: 一覧がRegistryから生成され、ショートカット未割当のCommandも載ること。Registryへ追加したCommandが、一覧側の変更なしで表示される統合テストを用意します。
- #109: menuとショートカットが同じIDのdispatchに合流すること、文脈による無効化、ID重複の拒否を確認します(既存の `tests/gui_command_registry.rs` を拡張します)。
- #200・#201: path欠落・衝突・到達不能の負例のfixtureで失敗すること、Windows・Ubuntuの両方で独立stepが実行され、集約check `rust` が失敗すること。
- #230: ひらがな・カタカナ・漢字がProportionalとMonospaceの両方で解決できるグリフ確認テスト。実画面の表示は、実GUIで確認するまで未確認として扱います。
- #6: Editの式、Viewの答え、Codeのraw CSVが同じ正本で食い違わないことを、実操作で確認します。

この文書だけでは実装は完了しません。実OS上のGUI操作は、CIの結果と区別して報告してください。

## 未決事項

1. **日本語フォントの選定。** 選択肢: (a) Noto Sans JPと等幅CJKフォントの2種を同梱、(b) 1種で両系統を兼ねる、(c) BIZ UDゴシック系。推奨は(a)です。理由は、表の桁揃えに等幅が必要で、同梱サイズより読みやすさを優先できるためです。いずれもライセンス本文を確認してから同梱してください。
2. **Altの単独押下の検出方法。** 選択肢: (a) eguiの `Modifiers::alt` の押下と離上を自前で追う、(b) OSのmenu mnemonicに任せる。推奨は(a)です。理由は、#113が「OSネイティブmenuへの全面依存」を非対象にしており、OSごとの差を避けられるためです。WindowsでAlt離上がシステムmenuを起動しないかは、実機で確認が必要です。
3. **モード切替のtop-level label。** 選択肢: (a) `V`(表示)の下に集約、(b) 専用の `M`。推奨は(a)です。理由は、Issue #113の例が `V` を表示としているためです。最終のlabelは、Registryの衝突検証で確定してください。
4. **行・列選択のキー。** 選択肢: (a) Shift+Space/Ctrl+Space(Excel互換)、(b) 別キー。推奨は(a)です。理由は、#110が「Windowsの一般的なSpreadsheet慣習を優先」としているためです。日本語IMEが有効なとき、Ctrl+Spaceと衝突しないかを確認してください。
5. **Command Paletteの起動キー。** Issue本文は「マウスなしで開ける」とのみ書いており、キーを指定していません。推奨はCtrl+Shift+Pです。理由は、エディタ系の一般的な慣習のためです。
