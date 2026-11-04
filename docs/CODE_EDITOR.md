# Codeモード編集器の設計契約

## 目的

CodeモードはCSV正本を最も直接扱うためのテキスト編集UIです。

ただし編集中のtextはApplyするまでDraftであり、壊れたCSVを一時的に入力できることと、正本CSVへ壊れた状態を確定しないことを両立します。

## 絶対条件

1. Code bufferの編集中textを外部同期で黙って消さない。
2. invalid CSVをDraftとして保持できる。
3. invalid CSVをcanonicalへApplyしない。
4. Applyは1つのatomic編集として扱う。
5. CodeのUndoとcanonical CSVのUndoを混同しない。
6. 自動整形で利用者のtextを勝手に書き換えない。

## 2種類の履歴

### Code Draft Undo

文字入力、削除、貼り付け等のtext editor内履歴です。

Apply前はcanonical CSV historyとは別です。

### Canonical Undo

Apply成功時に、Code変更全体を1つのcanonical Undo単位として記録します。

```text
Codeで10回編集
 ↓
Apply Code
 ↓
canonical historyでは1回の編集
```

## invalid buffer

Code bufferは一時的にinvalid CSVでも構いません。

invalidな間:

- canonical documentは変更しない
- SaveはCode未適用として止める
- parse errorを表示
- raw text自体は保持

## CSV構文診断

Code editorはCSV parserから診断情報を受け取ります。

```text
ParseDiagnostic
├─ byte / char range
├─ physical line / column
├─ record candidate
└─ message
```

引用符内改行があるため、物理行番号とCSV record番号を同一視しません。

## セル位置との対応

bufferがparse可能なとき、各cellのraw text spanを取得できるようにします。

```text
ParsedCellSpan
├─ CellRef
├─ record index
├─ field index
└─ text range
```

これにより:

- Edit/ViewのcellからCodeの該当位置へ移動
- Code cursor位置からcell addressを表示
- parse error近辺のrecordを特定

ができます。

## quotingを理解する

単純な改行splitやcomma splitで位置を求めません。

```text
"hello,world",x
"multi
line",y
```

のようなCSVを正しく扱います。

## 外部変更

CodeDraftは編集開始時のbase snapshot / revisionを持ちます。

外部変更後はDivergedとなり、Apply時に3-way merge契約へ従います。

競合が1件でもあればCode由来の変更はcanonicalへ部分適用しません。

## 明示整形

CSVのquotingや改行を正規化したい場合は、明示的な `Codeを整形` Commandとして提供します。

自動保存やfocus移動を理由に整形しません。

整形はCodeDraftだけを変更し、Applyするまでcanonical CSVは変わりません。

## 検索

Ctrl+F等のCode検索は未適用bufferを対象にします。

表形式Searchとは別scopeです。

## 大規模CSV

Codeモードは全textを毎frame再構築しません。

初期実装が単純bufferでも、将来rope / line index等へ移行できるよう、CodeDraft modelとGUI widgetを分離します。

## コマンド

```text
code.apply
code.discard
code.undo
code.redo
code.goto_cell
code.format
code.search
```

全GUI操作はAltから到達可能にします。

## 状態表示

最低限:

```text
未変更
未適用
外部変更と分岐
競合
構文エラー
```

を区別します。

単に `Modified` だけでは不足です。

## テスト

- invalid CSVをbufferに保持できる
- invalid buffer Applyでcanonical不変
- Draft Undoがcanonical historyを変えない
- Apply成功がcanonical 1 Undo単位
- quoted comma / quoted newlineのcell spanが正しい
- cell→Code / Code→cell対応
- external syncでbufferを消さない
- explicit formatだけがDraft textを正規化

この日本語版をCodeモード編集器の正本とします。