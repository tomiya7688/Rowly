# 検索・置換の設計契約

## 目的

Rowlyの検索は正本CSVを安全に探し、置換は対象を事前に確定してから1つの編集操作として適用します。

## 検索と置換を分ける

検索は読み取り専用です。

置換は正本CSVを変更する明示操作です。

検索結果を表示しただけでdirtyにはしません。

## 検索対象

表形式検索の対象:

- 現在source
- logical table
- 現在selection
- Viewで表示中の行
- 明示した列

Codeモードの文字列検索は別です。

Codeでは未適用text bufferを検索し、Edit / Viewではparsed cellを検索します。

## 値の種類

表形式検索は既定でcanonical raw valueを検索します。

View上の表示文字列やfresh derived resultを検索する場合は、検索対象種別を明示します。

```text
Raw
Derived
Displayed
```

Displayed検索は読み取り用です。Displayed文字列をそのままReplace対象のcanonical valueとみなしません。

## 検索方式

1.0で:

- 完全一致
- 部分一致
- 前方一致
- 後方一致
- 大文字小文字を区別
- 大文字小文字を区別しない
- 正規表現

を提供できる設計にします。

canonical valueをUnicode正規化して書き換えません。検索上の比較処理と保存値は分離します。

## SearchResult

各結果は表示位置だけでなく元データへ戻れるidentityを持ちます。

```text
SearchResult
├─ source identity
├─ CellRef / provenance
├─ raw value
├─ match span
└─ view position if available
```

Sort / Filter中でも結果からphysical cellへ戻れます。

## 次を検索 / 前を検索

順序は検索開始時のscopeに従います。

View scopeならView順、raw source scopeならcanonical順です。

検索中にWorkspaceRevisionが変わった場合は、古い結果一覧を無条件に使わず再検索します。

## 置換計画

Replace Allは直接書き始めません。

```text
検索条件
  ↓
ReplacePlan
  ├─ target cell
  ├─ before
  └─ after
  ↓
全件validation
  ↓
StateStamp再確認
  ↓
atomic commit
```

一件でも入力規則違反・target消失・競合があれば既定では全件適用しません。

## scope

Viewで表示中の行だけを置換する場合、plan作成時にView rowをphysical provenanceへ解決します。

commit直前にView / sourceのStateStampが変わっていれば再計画します。

表示行番号をそのままwrite targetにしません。

## 複数source

logical table全体のReplace Allは複数physical CSVを変更できます。

この場合はSessionChangeSet / OperationIdで1操作として扱い、1回のUndoで全sourceを戻します。

片方だけ変更した途中状態を露出しません。

## header

header行を置換対象に含めるかは明示します。

既定ではデータ行だけを対象にし、header変更は別optionにします。

header置換はcolumn identityやView設定へ影響するため、変更通知でStructure扱いにして再解決を行います。

## preview

Replace All前に件数と差分を確認できます。

例:

```text
42 cells will change
B2  old -> new
B8  old -> new
...
```

previewは古いStateStampに対して作られた場合、確定時に再検査します。

## コマンド

```text
search.open
search.next
search.previous
replace.open
replace.one
replace.all
```

Ctrl+F / Ctrl+H等の専用shortcutに加え、Alt KeyTipsから必ず到達可能にします。

## Undo

Replace Oneは1操作。

Replace Allはscope全体で1操作です。

複数sourceでも1つのapplication-level Undo groupです。

## 性能

巨大CSVでは全SearchResultの文字列copyを避け、source identity / CellRef / match span中心に保持します。

正規表現検索で暴走しない実装を選び、利用者入力を評価言語として実行しません。

## テスト

- 検索でCSV不変
- Sort / Filter中の結果が正しいprovenanceへ戻る
- Replace Allのvalidation失敗で部分変更なし
- multi-source Replace Allが1回Undo
- header replaceで設定再解決
- StateStamp変更後の古いpreviewをそのままcommitしない
- Code検索がcanonical CSVを直接変更しない

この日本語版を検索・置換の正本とします。