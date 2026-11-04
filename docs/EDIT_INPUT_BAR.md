# Edit入力バー・値編集の設計契約

## 目的

表計算ソフトのformula bar相当の操作性を提供しつつ、Rowlyでは**値と式を同じ欄に混ぜません**。

## 2つの入力

```text
Value Bar       = CSV raw value
Calculation Bar = Calculation Binding expression
```

同じcellを選択していても別の状態です。

## Value Bar

選択cellのcanonical raw stringを表示・編集します。

長文、改行、quoted CSV field等も扱えます。

Value Barで `=A1+B1` と入力しても通常文字列です。

Calculation Bindingへ自動変換しません。

## Calculation Bar

選択targetにbindingがある場合、その式を表示します。

式が無い場合は空です。

derived resultはView側の答えであり、Calculation Barへ値として混ぜません。

## Edit / View / Code

### Edit

- Value Bar編集可能
- Calculation Bar編集可能

### View

- raw value確認可能
- derived result表示
- 原則編集不可

### Code

- Value BarはCode buffer中のcurrent parsed valueを参考表示可能
- canonical Value Bar編集は行わない

## commit

Value Bar編集はInlineCellDraftと同じDraft契約を使います。

Enter / 明示Applyでvalidation後にcommit。

EscでDraft破棄。

外部変更で黙って消しません。

Calculation Bar編集もrule draftとして扱い、apply時にbinding validationを通します。

## multi-selection

複数cell selection時:

- 全値同じならValue Barへその値を表示可能
- 異なる場合は `<multiple values>` 等の非値表示
- typingした場合に全selectionへ同値設定するかは明示Commandで行う

曖昧な1cell編集として勝手に一括変更しません。

## keyboard

- F2相当: Value編集開始
- Calculation Bar focus command
- Value Bar focus command
- Esc
- Enter

具体shortcutはCommand Registryで管理します。

Alt経由でも必ず到達可能です。

## selection移動

未commit Draftがある状態でselectionが変わる場合:

- validならcommitして移動
- invalid/conflictなら移動を止めるか明示選択を求める

入力値を黙って捨てません。

## status

Value / Calculation / DerivedをUI上で明確に区別します。

```text
値: 1200
式: B2 * C2
答え: 1200
```

同値でも別概念です。

## テスト

- `=A1+B1` がValueでは文字列
- Calculation Barだけがbinding編集
- external changeでDraft保持
- invalid validationでvalue保持
- multi-selection曖昧表示
- keyboard-only editing

この日本語版をEdit入力バーの正本とします。