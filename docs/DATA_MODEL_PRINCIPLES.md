# CSV・計算・表示の設計原則

## 背景と目的

表計算ソフトでは、セルの値、計算式、書式、結合状態が一つのブックへ集まりやすく、データを通常のCSVとして取り出しにくくなります。RowlyはCSVをデータの正本に保ち、計算と表示を別の情報として扱います。

この文書は、Calculation Binding、計算結果の確定、View上の同値セル表示を設計・実装するときの基準です。READMEの基本原則と、各機能の詳細仕様はこの文書に従います。

## 基本モデル

```text
CSV
  └─ raw text values: canonical data

Project / DSL
  └─ calculation rules, validation, automation

View
  └─ derived results, formatting, visual grouping
```

- CSVには行とセルの文字列を保存します。Rowlyの機能を使わなくても、通常のCSVとしてデータを回収・編集できます。
- 計算規則、依存関係、実行条件はCSVの外に保存します。Project設定やDSLが失われても、CSVの値を復元できる状態を保ちます。
- 計算結果と表示設定はCSVから分離します。表示や再計算だけを理由にCSVを書き換えません。
- `.rwprj` はprojectの参照と設定を保持し、`.rowlyx` はproject treeを持ち運ぶpackageです。どちらもCSVに代わるデータ正本にはしません。

## 計算規則と結果

Excel形式の `=A1*B1` のような式を、CSVセルの特別な値として扱いません。CSV内の `=A1*B1` は、利用者が別途変換を指示しない限り通常の文字列です。

RowlyのCalculation Binding coreは、セルをtargetにしたpure expression、依存cell、triggerを保持し、依存順にderived resultを評価します。式ASTはcell read、scalar literal、算術演算、Abs / Min / Maxだけを表現できます。一般DSL runtimeを使わないため、自動計算からFile I/O、Project I/O、Import / Export、network、process実行は呼び出せません。target modelには矩形範囲と列相対ruleも表現できますが、このcoreでは未対応targetとして登録を拒否します。

計算結果はderived resultとしてViewへ渡します。依存値の変更、外部CSV同期後の変更、利用者の明示的なRecalculateを計算triggerにできます。計算が失敗した場合は元のCSV値を保ち、結果をerrorまたはstaleとして示します。

計算結果をCSVへ反映する操作はMaterializeとして明示します。Materializeは通常のCSV編集として検証・Undo・dirty stateの対象にします。通常の再計算やView表示ではMaterializeしません。

## View上の同値セル表示

canonical table modelにmerged cellを作りません。連続する同じnon-empty raw valueは、View上で一つのgroupのように描画できます。

- groupingは表示だけを変え、CSVの値、行数、セル座標、source provenanceを変えません。
- 表示をまとめるために下位セルを削除したり、空文字に置き換えたりしません。
- groupingの判定は表示文字列ではなくraw valueを使います。表示形式を適用した結果だけが同じ値をgroup化しません。
- 既存の空値は正当なCSV値として保持します。空値を自動groupingする必要はありません。
- grouping境界を跨ぐかどうかは明示した規則に従います。境界が曖昧な場合はgroupを分けます。

## 形式変換

XLSXはImport / Exportの交換形式です。書式、Excelの型、merged cell、計算式をRowlyのcanonical modelへ暗黙に取り込みません。

XLSXの式セルを取り込む機能を拡張するときは、計算済みの値をCSVの文字列として取り込みます。式をCalculation Bindingへ変換する場合は、変換可能な規則だけを利用者が明示して取り込めるようにします。変換できない式は警告し、式を実行可能な計算として黙って登録しません。

現在のExcel adapterは `data_only=False` で読み込み、式文字列をそのままCSVの文字列へ取り込みます。Rowlyはその文字列を評価せず、Calculation Bindingも生成しません。この現行動作は将来の式Import方針を満たしていないため、式の値Importや警告を追加する変更では、この文書に沿って仕様と利用者への説明を更新してください。

表示上のNumber / Date / Boolean解釈もCSV値へ自動反映しません。たとえばraw valueが `00123` の場合、Viewで `123` と表示してもCSVには `00123` を保ちます。

## 利用者の操作

現時点で利用者はCLIから次のコマンドでXLSXをImport / Exportできます。

```text
rowly excel import input.xlsx output.csv [sheet-name]
rowly excel export input.csv output.xlsx [sheet-name]
```

Calculation BindingのRust coreは実装済みですが、利用者向けGUI操作、DSLによる規則の永続化、Viewへの結果表示はまだ接続されていません。現時点で利用者がGUIやCLIからBindingを割り当てる手順はありません。process APIの呼び出し元は `CalculationEngine::set_binding` で規則を登録し、`recalculate_for_changes` または `recalculate_all` で結果を更新できます。結果は `CalculationEngine::result` から取得し、CSVのraw valueとdirty stateは変わりません。範囲・列相対target、Materialize、View groupingも未実装です。利用者向けの操作を追加するときは、規則を明示して結果をViewで確認し、必要な場合だけMaterializeを実行する流れを提供します。Codeは常にcanonical CSVのraw textを表示・編集します。

## 採用判断

Excel由来の機能を追加するときは、次をすべて確認します。

1. CSVを通常の表データとして回収できること。
2. 計算と表示の情報をCSV値・構造から分離できること。
3. 外部編集、diff、history、自動化との整合性を保つこと。
4. 表示や計算のためにraw valueを削除・空白化しないこと。
5. 利用者がデータを書き換える操作と表示だけの操作を区別できること。

この設計文書は日本語を正本とします。機能の現在の利用方法は [Excel連携仕様](EXCEL.md)、実装済み機能はコードと対応Issueを確認してください。
