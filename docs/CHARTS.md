# Chart可視化の設計契約

## 目的

表計算に必要なグラフを、CSVを変更しないView / Derived設定として提供します。

## 1.0基本Chart

- line
- bar
- column
- area
- scatter
- pie

複雑なChart互換は後回しにします。

## Data Source

Chart sourceは:

- raw range
- current View rows
- Group By result
- Pivot result
- fresh Calculation result

を選べます。

表示screen coordinateをデータidentityにしません。

## Series

```text
Chart
├─ category/x source
├─ series[]
│   ├─ name
│   ├─ value source
│   └─ style
└─ view options
```

sourceはstable column/range identityで保持します。

## freshness

sourceがstale/errorの場合:

- 古い値を正常Chartとして黙って描かない
- stale indicator
- missing/error pointsのdiagnostic

を持ちます。

## formatting

- title
- axis title
- legend
- number format
- series color

はView設定です。

CSVへ書き込みません。

## selection / drill down

Chart point選択から元row/provenanceへ移動できる構造にします。

aggregate pointの場合は元row集合を開けます。

## persistence

ProjectではView/Dashboard設定として保存。

bare CSVではsession-only。

## export

- image
- PDF/print
- XLSXへのchart exportは対応可能性を別途明示

Chart export成功でCSV dirtyをcleanにしません。

## keyboard

Chart作成・source変更・series選択・削除はCommand化しAltから到達可能。

## accessibility

Chartだけで意味を伝えません。

元データ / series summaryへkeyboardで到達できるようにします。

## テスト

- chart生成でCSV不変
- source identity再解決
- stale source表示
- aggregate drill down
- project reopen

この日本語版をChartの正本とします。