# 論理テーブル契約

複数CSVを物理結合せず、同一ヘッダーごとの論理テーブルとして結合表示し、列値ルールで論理シートに分割する設計です。

- 状態: 提案: Issue本文に基づく設計。利用者未承認
- 確認日: 2026-11-05
- 対象Issue: #1 複数CSVを論理テーブルとして結合表示する(親)、#9 GUIから列の値で論理シートを分割する(親)、#63 同一ヘッダーでの結合、#64 default書込先とsource間row移動、#65 列値ルールによる動的分割、#66 missing sourceを除外したdegraded再生成、#141 連続する同値セルの視覚結合
- 関連: #317(sheet tab相当の切替)、#318、#69(ExternalSync後のViewer設定の部分再解決)
- 読む人: 実装者

## 背景と目的

フォルダや`.rwprj`に同じ列構成のCSVが複数あると、利用者はファイルごとに開き直す必要があります。逆にCSVを物理結合すると、元ファイルが正本でなくなります。

そこでRowlyは、CSVを別ファイルのまま保持し、ヘッダーが一致するCSVを1枚の論理テーブルとして表示・編集します。さらに、列の値ごとに論理シートを分け、同じ値が続くセルをView上で1つに見せます。いずれもCSVへ新しい情報を書き込まず、表示と書込先の対応だけを扱います。tab切替の操作は[WORKSPACE_NAVIGATION.md](WORKSPACE_NAVIGATION.md)、View設定の全体は[VIEW_MODEL.md](VIEW_MODEL.md)、層の依存は[ARCHITECTURE.md](ARCHITECTURE.md)に従い、この文書はそれらと重複する説明を持ちません。

## 現行実装との差

確認したファイルは`src/logical_table.rs`、`src/project.rs`、`src/project_init.rs`です。`tests/`にも`src/logical_table.rs`内にも、論理テーブルを対象にしたテストは見当たりません(2026-11-05時点)。

| 項目 | 状態 | 根拠 |
| --- | --- | --- |
| ヘッダー一致でのtable化、行ごとのsource provenance、`display_order` | 実装済み・テスト未確認 | `LogicalProject::load`、`LogicalTable`、`LogicalRow`、`SourceRecord` |
| default書込先、行追加 | 実装済み・テスト未確認 | `set_default_write_target`、`resolve_write_target`、`append_row` |
| source間row移動とUndo/Redo | 実装済み・テスト未確認 | `move_row`、`undo_move`、`redo_move`、`transfer_csv_record`(backup付き) |
| missing sourceを除外した再生成 | 未実装 | `SourceStatus::Missing`は`src/project.rs`にあるが、`LogicalProject::load`は状態を見ずに全sourceを読み、存在しなければ`LogicalLoadError`で失敗する |
| 列値ルールによる動的分割 | 未実装 | 分割ruleの型が`src/`に存在しない |
| GUIからの分割操作 | 未実装 | 論理テーブル用のCommand IDが存在しない |
| 視覚結合 | 未実装 | View modelの`grouping`は[VIEW_MODEL.md](VIEW_MODEL.md)の設計のみ |

既知の隙間として、`LogicalTable::move_row`はdisplay orderを変えませんが、`RowMove`の`logical_row_index`は`rows`の位置であり、行追加・削除後のUndo安定性は検証されていません。

## 設計方針

### 守る不変条件

1. CSVはsourceごとに別ファイルのままとし、論理テーブルは新しい正本にしません。
2. 論理結合・分割・視覚結合のいずれも、CSV本文を暗黙に変更しません。CSVを変えるのは、利用者が明示した行追加・行移動・通常編集だけです。
3. 分割ruleと視覚結合ruleは、固定の行番号一覧を保存せず、CSVの現在値に対して都度評価します。
4. すべての操作はアプリケーションコマンドを通ります。GUI・CLI・DSLが別実装を持ちません。
5. 表示順(`display_order`)と書込先(`SourceRecord`)は独立です。並べ替えやフィルターは書込先を変えません。

### データモデル案

既存の`LogicalProject`/`LogicalTable`/`LogicalRow`/`SourceRecord`/`SourceFile`は維持します。追加する型を次に示します。

- `SourceAvailability`: `Available`または`Missing`。`LogicalTable.source_files`の各`SourceFile`に持たせるか、`LogicalTable.missing_sources: Vec<String>`で保持します(未決事項1)。
- `SplitRule`: `{ key_columns: Vec<String>, display_name: Option<String> }`。列は位置ではなくヘッダー名で参照します。
- `LogicalSheetId`: `SplitRule`の識別子と`key_values: Vec<String>`の組。空値・欠損値もkey値として扱います。
- `LogicalSheet`: `LogicalSheetId`と、評価結果の`row_indices: Vec<usize>`(`rows`のindex)。保存対象にせず、評価ごとに作り直します。
- `CellGroupRule`: `{ column: String, scope: GroupScope }`。`GroupScope`は既定で論理シート境界・source境界・フィルター境界を跨がない設定です。

### 処理の流れ

同一ヘッダー結合(#63)は現行の`load`を踏襲します。

1. 宣言されたsourceを列挙し、directoryはrecursive設定に従ってCSVを集めます。
2. ヘッダー(列名と順序)が完全一致するCSVを同じ`LogicalTable`へ入れます。
3. 各行へ`SourceRecord`(source id、path、data-record index)を付けます。

missing source(#66)では、`resolve_source_locations`の結果を`load`へ渡し、`Missing`と`Ambiguous`のsourceを読み込み対象から外します。除外したsourceのidと設定は保持し、再解決で`Available`または`Relinked`になれば同じsource idで再参加させます。

列値分割(#65)では、`SplitRule`のkey列の値の組ごとに`LogicalSheet`を作ります。行の追加・削除・値変更・並べ替え・外部変更の後に毎回再評価するため、所属は常に現在のCSV値と一致します。表示名は内部のkey値と分離し、利用者が変えても所属判定は変わりません。

視覚結合(#141)は、View描画時に、同じ列の連続する行でraw valueが完全一致する範囲をgroupとして計算します。

### 採用しなかった案

- 分割シートごとに行番号の一覧を保存する案: CSVの行追加・並べ替えで壊れるため、Issue #9の決定に従い採用しません。
- 論理結合時にCSVを物理的に1ファイルへ結合する案: 正本が増え、元ファイルの出自を失うため採用しません。
- 表示形式適用後の文字列で視覚結合を判定する案: `1.1`と`1.2`が`1`に見える場合に別値を誤って結合するため採用しません。

## 利用者から見た操作

Command IDは案です。確定は[COMMAND_REGISTRY.md](COMMAND_REGISTRY.md)の登録規約に従ってください。すべてAlt KeyTipsから到達できます。

| 操作 | Command ID案 | 入口 |
| --- | --- | --- |
| default書込先の設定・解除 | `logical.write_target.set` / `.clear` | GUI、CLI、DSL |
| 行のsource間移動 | `logical.row.move` | GUI(メニュー・Command Palette)、CLI、DSL。drag and dropは#64の対象外 |
| 列値で論理シートを分割 | `logical.split.by_column` | 列を選択し「この列の値でシートを分割」。複数列も指定可能 |
| 分割の解除 | `logical.split.clear` | 同上 |
| 同値セルの視覚結合の設定・解除 | `view.group.set` / `.clear` | Viewメニュー、文脈menu、Command Palette。keyboardのみで操作可能 |

失敗時は、CSVを変更せず、理由をエラー表示に出します。例は、target未指定、tableに属さないsource、同一source、列幅の不一致、移動元recordの変更です(`LogicalTableError`の各variant)。missing sourceは致命的エラーにせず、degradedであることと再linkの入口を表示します。視覚結合した範囲を選択すると、`Displayed group: 東京`と`Underlying: A2:A4`のように元のセル範囲を示します。

## 永続化と互換性

- CSV本文は変更しません。論理テーブル、分割、視覚結合の情報をCSVに埋め込みません。
- `.rwprj`はsourceの参照、`search_root`、`schema`を保持します([ARCHITECTURE.md](ARCHITECTURE.md)参照)。missing sourceのidと設定は削除しません。
- `SplitRule`、`CellGroupRule`、default書込先は、projectではgenerated init DSL(`generated.rly`)に保存し、`user.rly`は上書きしません。bare CSVではsession-onlyです。
- 論理シート内の行順やsheet表示名の保存は、tabの扱いに合わせて[WORKSPACE_NAVIGATION.md](WORKSPACE_NAVIGATION.md)の永続化に従います。
- 既存の`.rwprj`と`init.rly`は、新しい宣言が無くても従来どおり開けます。

## 安全性と失敗時の挙動

- source間moveは、移動元の現在値が読込時と一致することを確認してから、両CSVを更新します。現行の`transfer_csv_record`は事前にbackupを作ります。片方の更新が失敗したら、両方を元の状態に戻し、provenanceも更新しません。
- moveの移動先は、同じ論理テーブルに属し、現在のヘッダーが一致するsourceだけです。1つのsourceが複数CSVを提供している場合は、書込先が曖昧なため拒否します。
- 外部でCSVが変わってヘッダーが不一致になった場合は、操作を中止し、`SchemaChanged`として報告します。
- 分割は読み取り専用の再評価です。評価エラーは該当ruleだけを無効化し、他のViewとCSVに影響させません。
- 視覚結合は、空値を自動groupの対象にせず、別raw valueを結合しません。グループ化コマンド自体が空値を作ることはありません。
- 外部変更後の設定の再解決は#69に従い、壊れた項目だけをdormantにします。

## 実装順序と依存

1. #63: 現行`load`の受け入れ条件を、テストで固定します。先行条件はありません。
2. #66: `load`がavailableなsourceだけで再生成する処理と、再参加の処理を追加します。#63に依存します。優先度はP1です。
3. #64: 移動の原子性とUndoのテストを追加し、Command ID案を登録します。#63に依存します。#66と並行できます。
4. #65: `SplitRule`と評価を実装します。#63に依存し、#64・#66とは並行できます。#66の再評価の条件(残存rowでの再計算)は、#65の完成後に結合します。
5. #9(GUI): 分割のCommandとGUI入口を接続します。#65に依存します。
6. #141: View専用のgroup modelを実装します。先行は#60、#109、#113で、論理テーブルの実装とは独立して進められます。

## 受け入れ条件と検証

| Issue | 条件 | 検証 |
| --- | --- | --- |
| #63 | 同一ヘッダーの2CSVを1テーブルとして列挙し、行から元sourceへ辿れる。表示順を変えても出自を失わない | 単体、fixtureのCSVが別ファイルのままであることの統合テスト |
| #64 | default targetへ新規rowを書ける。moveで値を維持する。Undoで元sourceへ戻る。暗黙のCSV間移動をしない | 単体、失敗注入による部分更新なしの確認 |
| #65 | key値の変更で所属が変わる。外部追加rowも次の評価で入る。並べ替えで定義が壊れない。CSVにsheet情報が入らない | 単体、CSV不変のbyte比較 |
| #66 | 3source中1つが欠けても2つで開ける。saveで設定が消えない。復帰後に同じidで再参加する | 統合(`ProjectSession`のopen/save) |
| #9 | GUIから分割でき、DSLでも再現できる | GUI実操作、Alt KeyTipsのGUI実操作、`tests/gui_command_registry.rs`相当 |
| #141 | 連続する同値を1groupに描画し、raw不一致の同表示は結合しない。CSVは不変で、範囲を示す。設定が復元される | 単体、GUI実操作 |

Windows/Ubuntu CIは`.github/workflows/ci.yml`の該当checkに従います。

## 未決事項

1. missing sourceを保持する場所。選択肢: (A)`SourceFile`に状態を持たせる、(B)`LogicalTable`に別の一覧を持たせる。推奨はB。読み込み済みの行と欠落の情報を分離でき、既存の公開フィールドを変えないためです。
2. 分割後の論理シートでのrow追加先。選択肢: (A)default書込先を使う、(B)利用者に毎回選ばせる。推奨はA。Issue #64の既定動作と一致するためです。
3. 分割key列がヘッダーから消えた場合。選択肢: (A)ruleをdormantにして警告、(B)ruleを削除。推奨はA。[VIEW_MODEL.md](VIEW_MODEL.md)の外部変更の扱いと一致し、CSV復帰で再利用できるためです。
4. 視覚結合が分割シート境界を跨ぐか。選択肢: (A)常に跨がない、(B)ruleで選択可能。推奨はA。Issue #141が安全側での分割を求めており、1.0では選択肢を増やさないためです。
