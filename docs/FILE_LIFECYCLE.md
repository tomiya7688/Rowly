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