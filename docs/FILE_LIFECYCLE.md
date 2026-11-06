# New / Open / Save As ファイルライフサイクル契約

## 目的

Rowlyを起動してから新しいCSVを作成し、編集し、名前を付けて保存できる基本導線を定義します。

## New CSV

新規CSVは最初から勝手なtemporary fileを正本にしません。

session内に保存先未割当のcanonical documentを作ります。

```text
Backing
├─ Unassigned
└─ File(path, generation)
```

Unassignedでもcanonical table / Undo / validation等は通常どおり動作します。

## 初期table

無限空白sheetは作りません。

New CSV時に:

- 空table
- 初期row/column数
- optional header row

を明示して作成できるようにします。

1.0 GUI既定は小さな有限tableを作るか、0行0列から『行/列を追加』で始められる設計にします。

存在しない100万セルを仮想生成しません。

## first Save

BackingがUnassignedの状態で `file.save` を実行した場合、Save As相当のpath選択が必要です。

対話なしCLIではpath未指定ならInteractionRequired相当errorです。

## Save As

Save Asはcanonical contentを別pathへ保存し、そのpathを新しいbackingとして採用します。

成功時:

- canonical contentは同じ
- document/session identityは維持
- backing locator / generation更新
- dirtyをcleanへ

旧fileは削除しません。

## Save Copy

『コピーを保存』とSave Asを分けます。

```text
file.save_as      -> 新pathを今後のbackingにする
file.save_copy    -> copyだけ作り現在backingは変えない
```

Exportとも区別します。

## New during unsaved work

New CSV / New ProjectもTransition Guardを通します。

未適用Code、未保存CSV、Conflict等を無確認で破棄しません。

## path safety

Save As / Save CopyはPERSISTENCE_CONTRACT.mdへ従います。

- existing destination overwrite確認
- input alias safety
- temporary + atomic replace
- lossless-or-error

## New Project

Project作成はCSV作成とは別Commandです。

既存CSVをProjectへ追加する場合もsource identity / project rootを明示します。

## コマンド

```text
file.new_csv
project.new
file.save
file.save_as
file.save_copy
```

全GUI操作をAltから辿れます。

## テスト

- New CSVでdisk fileを勝手に作らない
- Unassigned documentの編集/Undo
- first Saveがpathを要求
- Save As後にnew backingへSave
- Save Copy後もold backing維持
- unsaved work中NewでTransition Guard

この日本語版をファイルライフサイクルの正本とします。

## 保存先未割当documentの実装契約（#332）

状態: 実装済み（`CsvDocument::new_unassigned` / `new_blank` / `backing_path` / `is_unassigned`）。確認日: 2026-11-06。

- 新規documentはBacking `Unassigned`で作り、diskにfileも一時fileも作らない。編集・Undo・Redo・検証は保存先があるdocumentと同じに動く。
- `new_blank(data_rows, columns, headers)`は行数・列数を明示した有限の空tableを作る。見出し行は任意で、個数は列数と同じでなければerror。0行0列も可。cell総数は100,000以下に限り、存在しない大量のcellを仮想的に作らない。
- `save` / `save_metadata`は保存先が無いと`DocumentError::SaveRequiresPath`で拒否する（対話なしでpathを自動で決めない）。拒否してもfileは作られず、編集内容とdirty状態は失われない。外部変更の確認は対象外（`refresh_if_external_change`は`false`）。
- `save_as(path)`で初めてbackingが決まり、canonical contentを書いてcleanになる。以後の`save`は新しいbackingへ書く。`path()`は保存先未割当のとき空のpathを返すので、保存先の有無は`backing_path()`（`None`なら未割当）または`is_unassigned()`で確かめる。
- 対象外: GUIの`file.new_csv`・path選択UI、Transition Guard、`file.save_copy`、既存の保存先を上書きする確認、New Project（後続Issue）。