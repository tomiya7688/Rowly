# Rowly DSL 1.0 標準関数

この日本語文書を Rowly DSL 1.0 の標準判定関数と名前空間の正本とする。

## 方針

ドメイン固有の判定関数をグローバルへ並べず、用途ごとの標準名前空間から呼び出す。

```text
Text.Contains(value, needle)
Text.StartsWith(value, prefix)
Text.EndsWith(value, suffix)
Text.IsJapanese(value)

Number.IsInteger(value)
Number.IsDecimal(value)

Boolean.IsValid(value)
```

関数名と名前空間名は ASCII 大小文字を区別しない。戻り値はすべて Boolean で、`If Text.IsJapanese(value) Then` のように条件へ直接使用できる。

## Text

### Text.Contains(value, needle)

2引数とも String を要求し、`value` が `needle` を含む場合に true を返す。暗黙の数値→文字列変換は行わない。

### Text.StartsWith(value, prefix)

2引数とも String を要求し、前方一致を判定する。

### Text.EndsWith(value, suffix)

2引数とも String を要求し、後方一致を判定する。

### Text.IsJapanese(value)

String を要求し、ひらがな、カタカナ、半角カタカナ、CJK 統合漢字・互換漢字の対象範囲を1文字以上含む場合に true を返す。CSV の値は変更しない。

## Number

### Number.IsInteger(value)

Integer runtime value は true。String は `i64` として明示的に解釈可能な場合に true。それ以外は false。値を Integer へ変換したり CSV を書き換えたりしない。

### Number.IsDecimal(value)

Integer / Decimal runtime value は true。String は有限な `f64` として解釈可能な場合に true。それ以外は false。

## Boolean

### Boolean.IsValid(value)

Boolean runtime value は true。String は ASCII 大小文字を無視して `true` / `false` の場合だけ true。それ以外は false。

`Boolean(...)` は既存の明示変換関数、`Boolean.IsValid(...)` は非破壊判定であり責務が異なる。

## 名前解決

`Text` / `Number` / `Boolean` は変数、関数引数、For ループ変数の予約 binding 名とする。

parser は `Text.Contains(...)` 等を user object の `MethodCall` ではなく標準名前空間専用 AST として生成する。runtime も専用の標準関数テーブルで解決するため、ユーザー class instance の method lookup と混在しない。

同名の class 定義自体は許可する。例えば `Class Text` の instance を `tool` に束縛した場合、`tool.Contains(...)` はユーザー method、`Text.Contains(...)` は常に標準関数として扱う。

## 旧グローバル関数

次の旧グローバル predicate builtin は Rowly DSL 1.0 では提供しない。

```text
Contains(...)
StartsWith(...)
EndsWith(...)
IsJapanese(...)
IsInteger(...)
IsDecimal(...)
IsBoolean(...)
```

同名のユーザー定義関数を作成すること自体は妨げない。ただしそれは標準関数ではなく、通常のユーザー関数として解決する。

## エラー

未知の標準関数は `Text.Unknown` のように名前空間と関数名を含む明示エラーにする。引数個数エラーも `Text.Contains` の完全名を報告する。

Text 系関数へ非 String を渡した場合は暗黙変換せず型エラーにする。Number / Boolean の判定関数は上記の判定規則に従い false を返す。

## 正本性

標準判定関数は値の意味を検査するだけで、CSV 正本文字列を変更しない。CSV への作用が必要な処理は引き続き process API を通す。
