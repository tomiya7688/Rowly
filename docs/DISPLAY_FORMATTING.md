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