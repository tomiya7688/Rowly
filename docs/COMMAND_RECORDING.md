# 意味ベースの操作記録・マクロ生成契約

## 目的

RowlyはGUI操作と自動化を別世界にしません。

マクロ記録はマウス座標やクリック列を録画するのではなく、**実行されたアプリケーションコマンドの意味を記録**します。

```text
GUI / Shortcut / Alt / Palette
            ↓
アプリケーションコマンド
            ↓
意味ベース記録
            ↓
Rowly DSL
```

同じ操作なら入口がマウスでもキーボードでも同じ記録になります。

## 絶対条件

1. mouse eventを記録しない。
2. GUI widget IDを記録しない。
3. 成功していないCommandを成功操作として記録しない。
4. 記録できない操作を黙って落とさない。
5. 再生時は記録時の権限を引き継がない。
6. 再生も同じアプリケーションコマンド境界を通る。

## 記録単位

Command定義は自動化可否を持ちます。

```text
記録可能
文脈依存で記録可能
記録不可
```

### 記録可能

引数を正規化すればRowly DSLへ安定して表現できる操作です。

例:

- cell / range編集
- 行列操作
- View設定
- Calculation Binding設定
- Filter / Sort設定

### 文脈依存

selectionやactive source等の現在文脈を具体的な引数へ解決すれば記録できる操作です。

記録時に曖昧な `現在選択中` のまま残さず、A1 rangeやsource_id等へ正規化します。

### 記録不可

UIだけの操作や再現性がない操作です。

例:

- Helpを開く
- popupを閉じる
- window resize

記録不可Commandを実行した場合、Recorderは『記録対象外』と明示します。黙って無視しません。

## 成功後に記録する

記録候補はCommand Requestですが、実際の記録はCommand Outcomeを確認した後に確定します。

```text
Request
 ↓
事前検査
 ↓
実行
 ↓
Outcome
 ↓
成功 / 意味のあるNoChangeだけ記録
```

失敗した操作をマクロへ残しません。

## 引数の正規化

記録内容はUI内部状態ではなく再実行可能な引数へ変換します。

例:

```text
selection=A1:A10
source_id=sales
column_header=price
value=100
```

pathを記録するCommandでは、絶対path / project-relative path等の意味を明示します。

## 機密情報

Command引数は記録可否とは別に機密区分を持てます。

```text
通常
path
secret
recording forbidden
```

password、token等をマクロへ記録しません。

## Rowly DSLへの変換

各記録可能Commandは、可能なら正規のRowly DSL表現を1つ持ちます。

GUI Recorder専用の隠しscript形式を作りません。

例:

```text
SET This.Worksheet.Editor.Cell("B2:B10").Value = "0"
```

同じDSLを手書きしても同じCommandへ到達します。

## Recorderの状態

```text
停止中
記録中
一時停止
記録に問題あり
```

記録に問題ありは、途中に記録不能Commandがあり完全再現できない状態です。

保存時にその事実を明示します。

## 保存

Projectでは:

```text
data/scripts/rowlydsl/macros/<name>.rly
```

へ保存できます。

bare CSVではsession-onlyを既定とし、利用者が明示的に保存先を選んだ場合だけscript fileとして保存します。

## 再生

再生は通常のRowly DSL runtime / Application Commandを通します。

記録時に許可されていたFile I/O権限等を再利用しません。

再生時のcapabilityで毎回判定します。

## 事前確認

マクロは実行前に可能な範囲で事前検査できます。

```text
parse
 ↓
Commandへ解決
 ↓
権限 / target / source / 引数を検査
 ↓
実行可能 / 追加確認 / エラー
```

これは実行の完全なsimulationではありません。

外部変更など実行時にしか分からない条件は実行時にも再検査します。

## Undo

データ変更だけで構成され、1つのapplication transactionへ安全にまとめられるマクロは、1回のUndo単位にできます。

Save / Open / Export等の境界操作を含む場合は1つのCSV transactionへ無理にまとめません。

1.0ではRecorderの既定対象をデータ編集・View設定・Calculation設定中心にし、Open / Quit等の遷移Commandは記録対象外でも構いません。

## GUI

- 記録開始
- 一時停止
- 停止
- 記録内容を表示
- DSLとして編集
- 名前を付けて保存
- 実行

を提供します。

全操作をAltから辿れます。

## テスト

- mouse / shortcut / Altから同じCommandを実行して同じDSLになる
- 失敗Commandは記録されない
- 記録不可Commandを黙って落とさない
- secret引数を記録しない
- 保存したDSLを再実行して同じデータ変更になる
- 再生時capability不足で正しく拒否される

この日本語版を意味ベース操作記録の正本とします。