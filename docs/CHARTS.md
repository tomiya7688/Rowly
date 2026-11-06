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

## Chart定義とdata source resolverの実装契約（#302）

状態: 実装済み（`src/process/chart.rs`、`src/process/chart/source.rs`）。確認日: 2026-11-06。

- `ChartDefinition{name, kind, source_id, category_header, series[]}`は、source idと列の見出しだけで持つ。表示座標や列indexを持たず、CSVには保存されない。種類はline / bar / column / area / scatter / pie。
- data sourceは`SourceTable{id, headers, rows}`に統一する。raw range・current Viewのrowは`from_input_rows`、Group By結果は`from_group_by`、Pivot結果は`from_pivot`（1つのValueを列keyごとの列にした幅広い表）で作る。どの行を入れるか（全row・View filter後・選択範囲）は呼び出し側が選ぶ。
- `resolve(definition, tables)`は現在のsourceへ引き直す。source idや見出しが無い・複数ある場合は、欠けたものを全部まとめて`ChartError::Unresolved`で返す。定義は捨てられず、sourceや列が戻れば再び解決できる。列の並びが変わっても結果は変わらない。
- stale・error・pendingのcellは通常のseriesとして描かない。空、数値でない値、categoryがfreshでない点、円グラフの負の値も欠損にし、理由（`MissingReason`）を点に残して件数を`ChartDiagnostics`に数える。`stale_indicator()`で古い値を含むことを示せる。
- 各点は`provenance`（元row参照）を持つ。aggregateの点はgroup / bucketに入った全rowで、drill downに使える。Pivotの空のbucketは元row参照が空の欠損になる。
- 円グラフはseriesが1つ、散布図はcategory列を数値のxに使う。seriesが無い、円グラフに複数series、散布図にx列が無い定義は`InvalidDefinition`。
- 解決はCSVに触れず、source表を変えてもCSVは変わらない。対象外: 描画（#303）、Projectへの保存（#304）、GUI（#305）。