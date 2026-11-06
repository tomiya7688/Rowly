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