# Project Command APIと設定の永続化

Projectのsource/resource操作を共通Command APIへ統一し、Project saveの原子性とViewer・入力規則設定のgenerated init DSL永続化を一貫して設計する文書です。

- 状態: 提案: Issue本文に基づく設計。利用者未承認
- 確認日: 2026-11-05
- 読む人: `ProjectSession`、`project_init`、`rowlyx` を変更する実装者
- 対象Issue:
  - #129 Project Command APIでsource / resourceの追加・削除・移動を統一する
  - #131 Project saveを `.rwprj` / `.rowlyx` で原子的に実行する
  - #60 Rowlyプロジェクトの generated init DSL にViewer設定を永続化する
  - #69 ExternalSync後にViewer設定を部分再解決して維持する
  - #107 DSL入力規則をproject init DSLとして保存・復元する
  - #108 Edit GUIから入力規則DSLを生成・編集できるようにする
- 関連Issue: #240（保存契約の設計トラッカー）、#236（Save Coordinator）、#334（New Project）、#206（Calculation Binding保存。generated.rly/user.rlyへの保存は実装済み）

保存契約の一般則（SaveReport、比較交換、部分成功、状態不明）は [PERSISTENCE_CONTRACT.md](PERSISTENCE_CONTRACT.md)、View設定の項目は [VIEW_MODEL.md](VIEW_MODEL.md) の「名前付きView」「外部変更」、New/Save Asの導線は [FILE_LIFECYCLE.md](FILE_LIFECYCLE.md) を正本とします。この文書はそれらを繰り返さず、Project固有の追加部分だけを定めます。

## 背景と目的

現在は、Projectを開いて保存する経路はありますが、Projectの構成を変更する経路がありません。GUI、DSL、CLIがそれぞれ独自にfileを移動・削除すると、manifest、CSV、履歴が食い違い、管理外のpathへ書き込む危険があります。また、Viewer設定と入力規則の多くは、Project再open後に復元されません。外部変更で列やsourceが無くなると、他の設定まで失われる懸念もあります。

この設計は次の4点を実現します。

1. source/resourceの追加・削除・移動を、GUI/DSL/CLI共通のProject Commandに集約します。
2. `.rwprj` と `.rowlyx` のProject saveで、壊れたmanifestや壊れたpackageを残しません。
3. Viewer設定と入力規則を、CSV本体へ書かずgenerated init DSL（`generated.rly`）へ保存・復元します。
4. ExternalSync後は、設定ごとに再解決し、参照が切れた設定だけを保留状態にします。

## 現行実装との差

確認したファイルは `src/project.rs`、`src/project_session.rs`、`src/project_session/configuration.rs`、`src/project_init.rs`、`src/project_init/*.rs`、`src/rowlyx.rs`、`src/logical_table.rs`、`src/gui/document_actions.rs` です。

| 区分 | 内容 |
| --- | --- |
| 実装済み | `RowlyProject` / `ProjectSource`（stableな `id`、`kind`、`path`、`schema`）のmanifest読み書き。`write_project_file_atomic` による一時file経由の置換 |
| 実装済み | `ProjectSession::open/save`。`.rowlyx` は一時workspaceへ展開し、`pack_project` で一時archiveを作って検証後に置換します。`ProjectFileSnapshot` と `ProjectInitSnapshot` によるmanifest・generated configの復元と、stagingの保持（`tests_staging.rs` に3件） |
| 実装済み | `ProjectDirtyState`（data / structure_or_config / package） |
| 実装済み | generated.rlyへの保存: 列型、入力規則（source ID＋一意header）、Calculation Binding。`user.rly` は上書きせず、無ければ空fileを作成します（`StagedProjectInit::commit`） |
| 実装済み | `LogicalTable::default_write_target` の概念（`src/logical_table.rs`）。ただしmanifestには保存されません |
| 未実装 | `AddSource`、`RemoveSource`、`DeleteSource`、`MoveSource`、`RelinkSource`、`SetDefaultWriteTarget`、managed resource操作、それらのUndo/journal。`ProjectSession` にsource変更APIはなく、呼び出し側が `project_mut()` で `sources` を直接書き換える構造です |
| 未実装 | resource変更とmanifest更新の間の失敗を戻すjournal。`SAVE This.Project` というDSL入口（`src/` に該当文字列なし） |
| 未実装 | Viewer設定（Filter、Sort、列layout、freeze、grouping）の保存。保存するのは列型・入力規則・Calculation Bindingだけです |
| 未実装 | 部分再解決とdormant。設定の宣言keyは `(source_id, header)` で、`ProjectInitConfig::apply_source` はヘッダー重複時に `AmbiguousHeader` でsource全体のopenを失敗させます。列が消えた場合、列型は適用をskipし、入力規則は `ValidationTarget::Header` として渡します |
| 未実装 | 入力規則のtarget拡張。現行はcolumnのみで、logical table identityとcell/range identityは未対応（#107の進捗欄と一致） |
| 未実装 | Edit GUIの入力規則編集。`src/gui` には候補選択（`validation_candidates_open`）はありますが、DSLを生成・編集する画面はありません |

`ProjectSession::save` はmanifest、generated config、archiveを順に確定し、失敗時に復元します。`CsvDocument::save` はProjectSessionの外にあり、GUIの `save_csv` が続けて `save_project_calculations` を呼びます。成果物ごとの結果を返す `SaveReport` は未導入で、#236が担当します。

## 設計方針

### 契約（守る不変条件）

1. CSV本文は設定保存で変更しません。設定はmanifestとscript（`init.rly`、`generated.rly`、`user.rly`）にだけ保存します。
2. Projectのsource/resource変更は、GUI/DSL/CLIから同じ `ProjectCommand` を通します。UI固有またはDSL固有のfs処理を追加しません。
3. 操作できる範囲は、manifestが宣言したmanaged worldに限ります。範囲外pathへの暗黙writeは拒否します。
4. `generated.rly` はアプリが全体を再生成して書き、`user.rly` は読み取りだけを行います。
5. 保存に失敗しても、staging、dirty state、復元可能な情報を保持します。

### データモデル案

```rust
enum ProjectCommand {
    AddSource { source: ProjectSource },
    RemoveSource { source_id: String },            // 参照解除のみ。物理fileは残す
    DeleteSource { source_id: String },            // 物理resource削除 + 参照解除
    MoveSource { source_id: String, to: PathBuf }, // source_idは維持
    RelinkSource { source_id: String, to: PathBuf },
    SetDefaultWriteTarget { source_id: Option<String> },
    CreateCsv { path: PathBuf }, CreateTextFile { path: PathBuf },
    WriteTextFile { path: PathBuf, text: String },
    MoveResource { from: PathBuf, to: PathBuf }, DeleteResource { path: PathBuf },
}
```

`ReadTextFile` は状態を変えないため `ProjectSession::read_text_file(path)` とします。`ProjectCommand` の実行結果は、逆操作 `ProjectInverse`（移動ならinverse move、削除前内容のblob退避、manifest差分）を返します。`DeleteResource` は削除前の内容をproject history領域のblobへ退避し、退避に成功した場合だけ削除します。

managed worldの判定は `ManagedPath::resolve(&ProjectSession, &Path)` に集約します。判定は `..` を字句的に除去し、symlinkを辿らず拒否し、`.rowlyx` ではworkspace内に限定します。`pack_project` が持つ `safe_relative_path` と同等の規則を共有します。

### 処理の流れ

1. 事前検査: managed world、存在、衝突、source ID重複を確認します。fileは触りません。
2. 準備: 変更後のmanifest案と逆操作を作ります。移動・削除の物理操作は、manifest案が妥当と分かった後に行います。
3. 確定: 物理操作 → session内manifest更新 → dirty記録の順で行い、途中で失敗したら逆操作で戻します。
4. 保存: `ProjectSession::save`（後述）が、manifest、generated.rly、archiveを一括で永続化します。

Command自体はProject saveではありません。DSLの `SAVE This.Project` とGUIのProject保存は、どちらも同じ `ProjectSession::save` を呼びます。

### 採用しなかった案

| 案 | 採用しない理由 |
| --- | --- |
| `project_mut()` で呼び出し側が直接manifestを編集する現行方式を維持する | 物理fileとmanifestの不整合、Undo情報の欠落を防げません |
| Viewer設定専用のsidecar（`*.view.json`等） | #60が別sidecarを禁止し、CSV横のsidecarは設定漏れの原因になります |
| 入力規則をJSON schemaで保存する | #107/#108が正本をDSLと定めています |
| 設定全体を再読込時に1件でもエラーなら全破棄する | #69の「他の有効設定を巻き添えにしない」に反します |

## 利用者から見た操作

Command ID案（登録は [COMMAND_REGISTRY.md](COMMAND_REGISTRY.md) に従い、Alt KeyTipsから到達できること）:

| 操作 | Command ID案 | 入口 |
| --- | --- | --- |
| sourceの追加・参照解除・物理削除・移動・再link | `project.source.add/remove/delete/move/relink` | GUIのProjectブラウザ、DSL、CLI |
| 既定の書き込み先設定 | `project.source.set_default_write_target` | 同上 |
| managed resource操作 | `project.resource.create_csv/create_text/write_text/move/delete` | 同上 |
| Project保存 | `project.save` | Ctrl+S（#236のCoordinator経由）、DSLの `SAVE This.Project` |
| 入力規則の編集 | `validation.rule.edit_allowed_values` / `validation.rule.edit_expression` | 列メニュー、Command Palette、Alt KeyTips |

`RemoveSource` と `DeleteSource` は確認ダイアログの文言と結果表示で必ず区別します。`DeleteSource` は物理fileを消すため、実行前に削除対象pathと退避先を表示します。

失敗時は、Commandが「何を、どの理由で拒否したか」を結果へ返します。たとえばmanaged world外のpathでは、対象pathと拒否理由（範囲外、symlink、衝突）を表示し、manifestと物理fileは変更前のままであることを示します。保存に失敗した場合は、失敗した成果物（manifest、generated.rly、package）を区別して表示し、未保存状態を保ちます。

入力規則のEdit GUIは、候補値リストまたはBoolean式を入力し、確定時に既存DSL parserで構文と型を検査します。不正な場合はエラー位置を示し、規則を確定しません。取消はstagingを変更しません。

## 永続化と互換性

| 設定 | 保存先 | 注記 |
| --- | --- | --- |
| source、script path、history path、default write target | `.rwprj` manifest | `default_write_target` はmanifest項目の追加が必要（未決事項1） |
| 列型、入力規則、Calculation Binding、Viewer設定、名前付きView | `generated.rly` | アプリが全体を再生成します |
| 利用者が手書きした宣言 | `user.rly` | アプリは読むだけで、保存で変更しません |
| 展開前の一式 | `.rowlyx` | 通常のproject treeとして保持されます。専用形式は作りません |

`init.rly` は `INCLUDE GENERATED` と `INCLUDE USER` だけを解釈する現行方針を保ちます。Viewer設定の宣言構文は、既存の `SET This.Project.Source("id")...` 形式を拡張します。解釈は `project_init` のsafe parserだけが行い、通常のDSL runtime、macro、File I/Oは実行しません。構文の詳細は実装時に [DSL_REFERENCE.md](DSL_REFERENCE.md) へ追記します。

入力規則は、既存の `Validation.AllowedValues` と `Validation.Expression` の宣言を維持します。Viewer設定は新しいkeyを追加するだけで、未知keyを持つ古いRowlyでのopenは「未対応設定を無視して警告」とします。この扱いは未決事項2で確定します。

bare CSVでは、Viewer設定と入力規則はsession-onlyで、Project用の保存処理を実行しません。ただし [ARCHITECTURE.md](ARCHITECTURE.md) には現行のCSV sidecar（`<csv>.rowly.json`）の説明が残っており、#60の「CSV横sidecarへ漏れない」と食い違う疑いがあります（報告事項）。この設計では、project内sourceでsidecarを読み書きしない現行動作だけを前提にします。

## 安全性と失敗時の挙動

- 原子性（`.rwprj`）: manifestは一時file＋置換で書きます。manifest、generated.rly、user.rlyの作成を順に確定し、失敗時は `ProjectFileSnapshot` と `ProjectInitSnapshot` で戻します。複数fileの電源断原子性は保証しません（[PERSISTENCE_CONTRACT.md](PERSISTENCE_CONTRACT.md) の「クラッシュ時の原子性」）。
- 原子性（`.rowlyx`）: 現在のworkspaceを新しい一時archiveへpackし、検証後にだけ元archiveを置換します。失敗時は元archiveを残します。archiveの置換時に比較交換（外部変更検知）を行うかは未決事項3です。
- resource操作のrollback: `ProjectCommand` の確定中に失敗した場合、逆操作を実行します。逆操作が失敗した場合は、該当成果物を状態不明として報告し、以後の破壊的操作を止めます（[SESSION_STATE_TRANSITIONS.md](SESSION_STATE_TRANSITIONS.md) の「状態不明」）。
- Undo: Project resource変更は履歴へ載せます。`DeleteSource` はblob退避、`MoveSource` はinverse move、`AddSource/RemoveSource` はmanifest差分で戻せます。Project saveは現在状態の永続化であり、履歴へ二重に載せません。
- 不正入力: 範囲外path、symlink、重複source ID、manifestを上書きするpathは拒否します。出力先と入力が同じ実体の場合は [PERSISTENCE_CONTRACT.md](PERSISTENCE_CONTRACT.md) の同一実体検出に従います。
- 部分再解決: 設定は項目ごとに独立して解決します。キーは `source_id`、logical table identity、一意のheaderの順に使い、path・行番号・表示順は恒久キーにしません。解決できない項目は、削除せずunresolved（保留）として保持し、`generated.rly` の再保存時にも保持します。対象が復帰し、同一性を確認できた場合だけ再適用します。ヘッダーが重複する場合はその項目だけを保留にし、sourceのopen全体を失敗させません。
- dirty state: Project command後は `structure_or_config` と、`.rowlyx` では `package` を立てます。終了前にこれらを検知できます。

## 実装順序と依存

| 順 | Issue | 最小単位 | 先行条件 |
| --- | --- | --- | --- |
| 1 | #131 | 現行saveの監査。resource操作用の逆操作とjournalの受け皿を追加 | なし |
| 2 | #129-a | `ManagedPath` と `AddSource/RemoveSource`（物理操作なし） | 1と並行可 |
| 3 | #129-b | `MoveSource/DeleteSource/MoveResource/DeleteResource` と逆操作 | 1、2 |
| 4 | #60 | Viewer設定のserialize/parse。まず1種類（freezeなど小さい設定）で通す | なし（2、3と並行可） |
| 5 | #107 | logical table identityとcell/range targetの拡張 | 4、`src/logical_table.rs` の識別子 |
| 6 | #69 | 部分再解決とdormant | 4、5（#63/#65/#66の統合状態を着手時に確認） |
| 7 | #108 | Edit GUI（column単位から） | 5、#109/#113 |
| 8 | #334 | New Projectが、この文書の初期構成を作る | 2。#334はこの設計と並行可 |

#240/#236のSave Coordinatorは、`project.save` を呼ぶ側の依存先です。この文書の `ProjectSession::save` を先に整えておけば、#236は結果型の導入に集中できます。

## 受け入れ条件と検証

| Issue | 条件 | 検証 |
| --- | --- | --- |
| #129 | GUI/DSL/CLIが同じCommandを呼べる | 統合テストで同じ `ProjectCommand` を3入口から実行 |
| #129 | Remove/Deleteの区別、`source_id` 維持、範囲外write拒否、Undo情報 | 単体テスト（symlink・`..`・`.rowlyx`を含む） |
| #131 | 保存失敗で壊れたmanifest/packageを残さない、dirty報告、再open一致 | 単体・統合テスト（既存の `tests_staging.rs` を拡張）、`.rowlyx` 往復 |
| #60 | 再open後の復元、`user.rly` 保持、sidecarなし | 統合テスト。bare CSVのfile生成なしを確認 |
| #69 | 1列削除・source欠損・列復帰で他設定が維持される | 単体・統合テスト |
| #107 | generated DSLにruleが残る、user.rly保持、`.rowlyx` 往復 | 統合テスト |
| #108 | 不正式の拒否、再open一致、Altのみで保存・取消 | 単体＋GUI実操作 |

GUI実操作の確認は、Windows/Ubuntuの必須CI（`.github/workflows/ci.yml`）とは別に報告します。CIが成功しても、GUI実操作の合格にはしません。

## 未決事項

1. `default_write_target` のmanifestへの保存方法
   - 選択肢: (a) manifestに `default_write_target` を追加する。(b) generated.rlyへ保存する。
   - 推奨: (a)。source構成と同じ編集単位で、`MoveSource` のmanifest更新と同時に整合を保てます。manifestの `PROJECT_VERSION` の扱いは実装時に確認します。
2. 未知のViewer設定を含むgenerated.rlyを古いRowlyで開く際の扱い
   - 選択肢: (a) 警告して無視する。(b) open全体を拒否する。
   - 推奨: (a)。設定は補助情報であり、CSV本文は常に開けるべきだからです。ただしuser.rlyの構文エラーは従来どおり拒否します。
3. `.rowlyx` 保存時のarchive外部変更の検知
   - 選択肢: (a) 保存時に元archiveの世代を比較して拒否する。(b) 現行どおり比較せず置換する。
   - 推奨: (a)。[PERSISTENCE_CONTRACT.md](PERSISTENCE_CONTRACT.md) の比較交換と一致させるためです。ただし世代の定義は #236 と合わせて決めます。
4. `DeleteSource` が退避するblobの保存先と保持期間
   - 選択肢: (a) project historyディレクトリ配下に保持し、history整理時に削除する。(b) OSのごみ箱へ移す。
   - 推奨: (a)。`.rowlyx` 内でも同じ手順を使え、OS依存の分岐をアプリへ散らさないためです。
5. Viewer設定のDSL構文（key名と値の表現）
   - 選択肢: 既存の `Worksheet.Column(...)` 配下へ追記する案と、`Worksheet.View("name")` 配下へまとめる案。
   - 推奨: 名前付きViewは `View("name")` 配下、列単位の設定は `Column(...)` 配下です。[VIEW_MODEL.md](VIEW_MODEL.md) の `ViewProfile` と対応させるためです。ユーザーの確認後に確定します。
