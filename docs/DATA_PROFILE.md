# データプロファイル・品質確認の設計契約

## 目的

CSVを開いたとき、値を書き換えずに『この列に何が入っているか』『空欄がどれだけあるか』『入力規則違反があるか』を確認できる機能を提供します。

データプロファイルは読み取り専用です。

統計結果や推奨設定をCSVへ自動反映しません。

## 基本統計

列ごとに:

- 行数
- empty count
- non-empty count
- distinct count
- duplicate count
- 上位頻出値
- 最長 / 最短文字数
- 明示型がある場合のmin / max
- parse failure count
- validation violation count

を計算できます。

## raw値を基準にする

distinctや頻出値は既定でcanonical raw valueを使います。

表示形式で同じに見えるだけの値を同一値にしません。

例:

```text
00123
123
```

はraw profileでは別値です。

## 型

Rowlyはプロファイルのためにcanonical typeを推論して固定しません。

### 明示型あり

既存のcolumn type設定を使ってNumber / Boolean等の統計を計算します。

### 明示型なし

文字列統計を基本にし、必要なら『数値として解釈できる割合』等を参考情報として表示できます。

この参考情報から型を自動設定しません。

## empty

空文字は正当なCSV値です。

empty countを表示しても削除・補完しません。

NULLのような別概念を勝手に導入しません。

## 複数source

logical tableでは:

- 全体
- source別

を切り替えて確認できます。

全体統計でも各値のprovenanceを失いません。

## Viewとの関係

プロファイルscopeを選べます。

```text
全canonical rows
現在Viewで表示中のrows
selection
```

View scopeの統計はFilter変更で変わりますが、CSVは変わりません。

## Calculationとの関係

raw column profileとderived result profileを明示的に分けます。

derived resultはfreshな値だけを通常統計へ入れ、stale/error/pending件数を別表示します。

古い計算値を正常値として統計へ混ぜません。

## 入力規則

既存validation reportと統合し:

- violation count
- violation values
- affected rows

を確認できます。

プロファイルから『この値集合をAllowedValuesにする』等を提案する機能は将来追加できますが、必ず明示Commandとして適用します。

## 操作

頻出値を選んで:

- Filter条件へ追加
- Searchする
- 該当rowへ移動

できます。

これらは共通Commandを呼びます。

## キャッシュ

プロファイル結果はDerived Stateです。

WorkspaceRevision / source DocumentRevision / View profile revisionに紐付けます。

revisionが変われば必要範囲だけ無効化します。

## 性能

大規模CSVではstreaming aggregateを基本にし、全値のcloneを要求しません。

distinct countが巨大になる場合はexact / approximateを将来分けられる設計にします。

1.0ではexactを基本とし、上限に達した場合は『計測省略』を明示しても構いません。黙って近似値をexactとして表示しません。

## コマンド

```text
profile.open
profile.scope.set
profile.refresh
profile.value.filter
profile.value.search
```

全操作はAltから到達可能にします。

## テスト

- profile計算でCSV不変
- raw distinctで00123と123を分ける
- explicit numeric typeでmin/max
- parse failureを別count
- validation violation集計
- stale derivedを通常統計へ混ぜない
- source別 / logical全体scope

この日本語版をデータプロファイルの正本とします。