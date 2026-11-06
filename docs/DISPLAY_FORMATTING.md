# 表示書式・条件付き書式の設計契約

## 目的

表計算に必要な数値・日付・割合・配置・折返し・罫線・色等を、CSV値から完全に分離したView設定として提供します。

## 絶対条件

1. 書式変更でCSV raw valueを変更しない。
2. 表示形式を理由に数値へ自動変換しない。
3. 罫線・背景色・文字色をCSVへ埋め込まない。
4. conditional formattingは表示だけを変える。
5. 書式が失われてもCSVデータは完全に回収可能。

## Display Format

1.0で必要:

- Text
- Integer
- Decimal
- Percent
- Date
- DateTime
- Boolean
- Custom

raw valueは常にStringです。

例:

```text
raw: 0.125
View format: Percent(1 decimal)
display: 12.5%
```

CSVには `0.125` のまま保存します。

## Parse failure

表示形式に解釈できないraw valueは勝手に書き換えません。

例:

```text
format = Number
raw = abc
```

Viewではformat errorとして示し、raw `abc` を保持します。

## Alignment / Wrap

View設定として:

- left / center / right
- vertical alignment
- wrap
- truncate / ellipsis

を持てます。

## Row height

列幅と同様、行高もView settingです。

auto heightはwrap後の表示に応じて計算してよいですが、CSV構造へ影響しません。

## Border

borderは表示上のedge設定です。

```text
top
bottom
left
right
diagonal_down
diagonal_up
```

を同じCommand modelで扱います。

**斜め罫線だけkeyboard経路が無い、という設計は禁止します。**

全border操作はCommand Registryへ登録しAltから到達できます。

## Font / Emphasis

View設定として:

- bold
- italic
- underline
- text color
- background color

を持てます。

OS font fileをprojectへ勝手に埋め込みません。

## Conditional Formatting

条件とstyleをCSV外に保持します。

例:

```text
IF price > 1000 THEN background = red
```

### 評価対象

- raw value
- explicit typed interpretation
- fresh derived result

stale/error derived resultを通常値として条件評価しません。

## 条件付き書式の優先順位

複数ruleが一致する場合の優先順位を明示します。

1. rule order
2. propertyごとの最後の明示値

等、決定的な規則を持たせます。

同じ入力でframeごとに色が揺れないようにします。

## scope

書式ruleは:

- cell
- range
- column
- logical table

をtargetにできます。

projectではstable identityを使い、display row numberを永続keyにしません。

## 永続化

Projectではgenerated init DSLへ保存。

bare CSVではsession-only。

missing column/sourceはdormant設定として保持し、他の書式をresetしません。

## Copy

通常Copyはraw valuesだけです。

View styleを複製する場合は `view.format.copy` / `view.format.paste` のような専用Commandにします。

raw value pasteとstyle pasteを混ぜません。

## Export

canonical CSV exportにはstyleを含めません。

XLSX / PDF / displayed View exportでは、対応可能なstyleだけを明示的に反映できます。

表現不能styleはlossless-or-error / diagnostic契約へ従います。

## テスト

- Number/Percent/Date formattingでCSV不変
- parse errorでraw値保持
- border全種類がCommand経由
- conditional formattingでCSV不変
- stale derivedを条件に通常評価しない
- project reopenで復元

この日本語版を表示書式・条件付き書式の正本とします。

## DisplayFormat coreの実装契約（#284）

状態: 実装済み（`src/process/display_format.rs`）。確認日: 2026-11-07。

- `format_value(raw, &DisplayFormat) -> Display`は、rawを表示文字列にする純粋な関数で、`CsvDocument`を受け取らない。CSVのraw valueは変わらない。書式はText / Integer / Decimal / Percent / Date / DateTime / Boolean / Customで、View設定であり、CSVには保存されない。
- rawは常に文字列で、表示形式を理由に数値などへ自動変換しない。Textはrawをそのまま見せる（`00123`の先頭の0を保つ。数字に見える文字列も解釈しない）。
- 数値は10進の桁の文字列のまま扱い、浮動小数点の誤差を持ち込まない。丸めは絶対値で5以上を切り上げ、桁上がり（`9.995`→`10.00`）も正しく、丸めて0になった値に負の符号を付けない。Integerは小数部が0以外のrawを黙って丸めず`NotAnInteger`にする。Percentはrawを100倍して`%`を付ける（`0.125`→`12.5%`）。
- rawの読み方はlocale非依存の正規形だけ: 任意の符号、整数部1桁以上、小数点を付けるなら小数部1桁以上のASCII数字。「1,234」「1.234,5」「1e3」「.5」「5.」「全角数字」は読まず、曖昧なまま正規形にしない。前後の空白は読む時だけ許す。日付は`YYYY-MM-DD`（存在する日。うるう年を判定）、日時は`YYYY-MM-DDTHH:MM[:SS]`または空白区切りで、時差の指定は読まない。真偽は`true`/`false`（大文字小文字は問わず、`1`/`0`/`yes`は読まない）。日付の表示様式（ISO・スラッシュ・日本語・日月年・月日年）は表示だけを変える。
- 書式に読めないrawは書き換えず、`Display::Error{shown: raw, error}`で、rawをそのまま示し、理由（`FormatError`）を添える。空のrawは値が無いので、どの書式でも空の表示（errorにならない）。
- Customは`CustomFormatRegistry`（idでrawを整形する登録先）に任せる拡張の境界で、coreは中身を知らない。未登録は`UnknownCustom`、整形の失敗は`CustomFailed`で、いずれもrawを保つ。`format_value`は登録先なしとして扱う。
- 対象外: 配置・罫線・フォント・条件付き書式（#285・#286）、書式のProjectへの保存（#287）、GUI（#288）、ExcelやlocaleのCustom書式文字列の解釈。

## View style modelの実装契約（#285）

状態: 実装済み（`src/process/view_style.rs`）。確認日: 2026-11-07。

- `ViewStyle{rules, row_heights}`は、配置・折返し・罫線・太字等・色・行高のView設定で、`CsvDocument`を受け取らずCSVには触れない。`CellStyle`の各項目は「未指定（None）」を持ち、未指定は他のstyleを上書きしない。項目は水平配置、垂直配置、折返し・切り詰め・省略記号、太字・斜体・下線、文字色・背景色（`#rrggbb`、小文字へ正規化）、罫線。
- 罫線は`BorderEdge::ALL`の6方向（上・下・左・右・斜め2方向）を同じ型、同じ`set_border`で扱う。斜め罫線だけ別の経路にしない。辺ごとに`Line`（線種と色）または`Clear`（明示的に消す）を指定でき、後の指定が先の指定を上書きする。キーボード操作などの網羅は`BorderEdge::ALL`を正本にする。
- 統合は決定的: `resolve`した後、`style_for(row, column)`が有効なruleを入力順にたどり、項目ごとに後の明示値が勝つよう重ねる。順序を入れ替えれば結果が変わる（順序が意味を持つ）。同じ入力から常に同じ結果。条件付き書式（#286）はこの統合の上に重ねる。
- targetはstable identityで持つ: 列は見出し、行はCSVの行位置（表示順ではない）。種類は`Column`・`Cell`・`Range`（列の集合と行の範囲）。現在の見出しへ引き直すので、列の並びが変わっても同じcellに効く。
- 見出しが無い、または複数あるruleだけが`resolve`で休眠（`DormantRule{missing, ambiguous}`）になり、他のruleは有効のまま。休眠したruleは捨てず、見出しが戻れば再び有効になる。
- `style_for`は1 cellあたり有効なruleの数に比例する計算量なので、大きな表では表示するcellだけを解決して呼ぶ（全cellを走査しない）。範囲に同じ見出しが重複しても、解決後の列位置は1回だけ持つ。
- 行高は行位置をキーにした正で有限（上限1,000mm）の値で、CSV構造に影響しない。`add_rule`は空のtargetや逆の行範囲を、`set_row_height`は不正な行高を拒否する。
- 対象外: 表示書式の適用（#284）、条件付き書式（#286）、Projectへの保存（#287）、GUI・Command登録（#288）、描画。

## 条件付き書式rule engineの実装契約（#286）

状態: 実装済み（`src/process/conditional_format.rs`）。確認日: 2026-11-07。

- `ConditionalFormat{rules}`は、条件に合うcellへ表示styleを重ねるView設定で、`CsvDocument`を受け取らずCSVには触れない。評価（`ResolvedConditional::evaluate`）は純粋な関数で、File I/Oなどの副作用を持たず、同じ入力から常に同じ結果になる。
- ruleは`ConditionalRule{target, source, predicate, style, stop_if_true}`。targetはView style（#285）と同じ`StyleTarget`で、同じ規則で見出しとCSVの行位置へ引き直す（`resolve_target`を共有）。見出しが無い・重複するruleだけが休眠し、他は有効のまま。休眠したruleは捨てず、戻れば再び有効になる。
- 値の取り方（`ValueSource`）: `Raw`（raw文字列。比較は辞書順で、"10"は"9"より小さい）、`Number`・`Date`（rawを正規形の数値・`YYYY-MM-DD`の日付として明示的に解釈）、`DerivedText`・`DerivedNumber`（freshな計算結果）。数値は10進の桁のまま正確に比べる（浮動小数点で区別できない桁も）。右辺の型が値の取り方に合わないruleは、`add_rule`で拒否する。
- 条件（`Predicate`）: 比較（=・≠・<・≤・>・≥）、範囲（両端を含む）、含む・始まる（文字列の値だけ）、空・空でない、And・Or・Not。空の組み合わせや空の検索語は拒否する。
- 評価できない場合は合ったことにせず、理由を`Diagnostic`として返す。数値・日付として読めない値（「abc」「1,234」「1e3」「存在しない日」）は`NotANumber`・`NotADate`で、0や空として扱わない。空のcellは数値・日付の比較に合わず、診断も出さない。
- stale / error / pendingの計算結果、計算結果が無いcellは、通常の値として評価せず（条件に合う値でも）、`DerivedStale`・`DerivedError`・`DerivedPending`・`DerivedMissing`の診断にする。freshな計算結果だけが評価される。
- 優先順位はruleの順序: 先のruleから評価し、合ったruleのstyleをView styleと同じ規則（項目ごとに後の明示値が勝つ）で重ねる。`stop_if_true`のruleが合ったら、後のruleは評価しない。順序を入れ替えれば結果が変わる。
- 対象外: Projectへの保存（#287）、GUI・Command登録（#288）、色の自動配色（色階調・データバー）。

## 表示設定のProject保存の実装契約（#287）

状態: 実装済み（`src/project_init/display_views.rs`、`src/process/display_settings.rs`）。確認日: 2026-11-07。

- 保存するのは設定だけ: 列の表示書式（`DisplayFormat`）、View styleのrule（`StyleRule`）、行高、条件付き書式のrule（`ConditionalRule`）。表示結果やCSVの値は保存しない。CSVにもsidecar fileにも書かない。
- 保存先はgenerated init DSL。1項目1行で、`SET This.Project.Source("id").Worksheet.Display.<Format|RowHeight|Style|Conditional>("key") = ..`の形。keyは、Formatは列の見出し、RowHeightはCSVの行位置、Style・Conditionalは順序を表す通し番号（0始まり、読み戻しでは数値順）。値は既存のDSL値（文字列・リスト・名前付き呼び出し）だけで、数値も引用符つきの文字列で持つ。
- 書式: `Text()`、`Integer(Thousands(..))`、`Decimal(Places(..), Thousands(..))`、`Percent(Places(..))`、`Date(Style(..))`、`DateTime(Style(..), Seconds(..))`、`Boolean(True(..), False(..))`、`Custom("id")`。styleは指定した項目だけを書き、罫線は6方向とも同じ`Border("辺", "線種"[, "色"])`・`Border("辺", "Clear")`で書く。
- 読み戻しは、書式・style・行高・条件付き書式をそれぞれのmodelの検証（`add_rule`・`set_row_height`）に通す。文法や検証の誤りは、行番号つきのerrorにして、黙って捨てない。
- 列は見出し名で持つので、保存時に現在のCSVに列があるかは見ない。解決時（`DisplaySettings::resolve`）に、見出しが無い・重複する設定だけが休眠し、他の設定はそのまま有効になる。休眠した設定は保存にも残り、見出しが戻れば再び有効になる。
- `user.rly`は読むだけで書き換えない。generatedとの優先は、列の表示書式と行高はキー（見出し・行位置）ごとにuser側が勝つ。style ruleと条件付き書式は順序が意味を持つので、userが1つでも定義したsourceではそのカテゴリのgeneratedを使わず、保存もしない。
- `ProjectSession::save_generated_display_settings`は、検証に通らない設定（見出しの重複・空、不正なstyle・条件付き書式・行高）や不明なsourceを、stagingの前に拒否する。stagingは`save`までdiskに出ず、保存に失敗してもstagingは残って再試行できる。bare CSV（projectが無い）では使えず、設定はsession内だけに留まる。
- `DisplaySettings::resolve`と`ResolvedDisplay::render_cell`は、表示書式で整形した文字列と、View styleの上に条件付き書式を重ねたstyleを一度に返す。条件付き書式の評価はrawと計算結果から行い、整形後の文字列は使わない。
- 対象外: GUI・Command登録（#288）、structure edit（行・列の挿入・削除）に追随する設定の付け替え（#343）。