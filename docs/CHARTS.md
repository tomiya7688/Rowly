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
- 円グラフはseriesが1つ。散布図はcategory列を数値のxに使い、共通のxに複数のy seriesを置ける。xは実在してfreshな数値のcellだけで、cellが無い行を行番号で代用しない。診断の件数はseries毎の点を数える。点の来歴は値のcell自身のものだけで、値の無い点に別cellの来歴を貸さない。seriesが無い、円グラフに複数series、散布図にx列が無い定義は`InvalidDefinition`。
- 解決はCSVに触れず、source表を変えてもCSVは変わらない。対象外: GUI（#305）。描画は下の「Chart描画の実装契約（#303）」で定める。Projectへの保存は下の「Chart定義の保存・復元の実装契約（#304）」で定める。

## Chart定義の保存・復元の実装契約（#304）

状態: 実装済み（`src/project_init/chart_views.rs`、`ProjectSession::save_generated_chart_views` / `chart_views`）。確認日: 2026-11-06。

- projectでは`generated.rly`へ1定義1行で保存する: `SET This.Project.Source("id").Worksheet.Chart("name").Definition = Column(Source("data"), Category("month"), Series([Series("sales", "Sales")]))`。外側の`Source("id")`はChartの元になるprojectのsourceで、`Source(..)`引数が`ChartDefinition.source_id`（解決時に引くdata source）である。種類はLine / Bar / Column / Area / Scatter / Pie。
- `ProjectSession::save_generated_chart_views`で他の設定と同じstagingに載り、`save`で確定する。保存に失敗してもstagingは残り、再試行できる。再openは`chart_views`で名前順に読む。壊れた宣言は黙って捨てず、行番号つきのerrorにする。
- `user.rly`は読むだけで書き換えない。同じsource・同じ名前の定義が`user.rly`にあれば、それを優先し、generatedには書かない。
- 保存時にdata sourceや列が存在するかは検証しない。sourceや列が欠けても定義は消えず、`chart::resolve`が`Unresolved`を返すだけ（休眠）。戻れば再び解決できる。名前の空・重複、data source idの空は保存時にerror。
- `ChartDefinition.source_id`は物理source、named View、Group By / Pivot結果などを区別する文字列で、そのまま保存し、意味は解決する側（`SourceTable`のid）が決める。たとえばnamed Viewなら呼び出し側が`view:<名前>`のような安定したidで`SourceTable`を作る。
- bare CSVにはprojectが無いため保存しない（session内だけ）。保存するのは定義だけで、描画結果やCSV値は保存しない。

## Chart描画の実装契約（#303）

状態: 実装済み（`src/process/chart/render.rs`、`src/process/chart/render/shapes.rs`）。確認日: 2026-11-06。

- `render_svg(&ResolvedChart, &ChartStyle)`が、解決済みChart（#302）を決定的なSVG文字列にする。GUIや外部crateに依存せず、CSVには触れない。同じ入力は常に同じ文字列になる（座標は小数2桁、乱数・時刻なし）。
- 6種類: line（折れ線と点）、area（面と折れ線）、column（縦棒）、bar（横棒）、scatter（散布）、pie（円）。
- `ChartStyle`はView設定（CSVに保存しない）: title、軸title、legendの表示、series色（`#rrggbb`。足りない分は色覚に配慮した既定8色を順に使う）、数値書式（小数桁・3桁区切り）、大きさ（200〜10,000px）。不正な大きさ・色は`RenderError`。文字はXMLとしてescapeする。
- stale / error / pendingの点は値の点とは別の印で示す: stale=橙の菱形、error=赤い×、pending=灰色の白抜き丸（`data-reason`属性つき）。freshでない点を含むときは、図の上部に件数つきの警告文を書く。freshでない値は、通常の点・棒・扇形としては描かない。空・数値でない値・円グラフの負の値は印も出さず、診断の件数にだけ残る。
- 巨大なseriesの方針: line / area / scatterは1 seriesあたり2,000点（`MAX_POINTS_PER_SERIES`）へ、先頭と末尾を残して等間隔に間引く。column / bar / pieはcategory 200件（`MAX_CATEGORIES`）まで描き、超える分は描かない。省いた件数は`RenderedChart.omitted_points`で報告する。SVGの大きさはこの上限で頭打ちになる。
- 画像だけで意味を伝えない: SVGに`role="img"`・`<title>`・`<desc>`（seriesごとの点数・最小・最大の要約）を持たせる。元データと要約へのkeyboard到達はGUI側（#305）の対象。
- 対象外: ラスタ画像・PDFへの変換（#309）、GUIでの表示・操作（#305）、複数軸・積み上げなど複雑なChart。