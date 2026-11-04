# ワークスペース・テーブルナビゲーションの設計契約

## 目的

複数CSV、logical table、分割sheet、名前付きViewを、表計算ソフトのsheet tabに近い操作感で切り替えられるようにします。

ただしtab自体をデータ正本にはしません。

## ナビゲーション対象

```text
Project
├─ Physical Sources
├─ Logical Tables
│   └─ Logical Sheets
└─ Named Views
```

各対象は既存のstable identityを参照します。

## tab

tabはUI上のnavigation shortcutです。

tab順を変更してもCSVやProject source順を暗黙変更しません。

tabを閉じてもsourceを削除しません。

source削除はProject Commandの別操作です。

## tab種類

見た目で区別します。

- physical source
- logical table
- logical split sheet
- named View

同名でもidentityで区別します。

## active target

active tabはUI stateです。

active targetが変わるとselection / View / Code contextを対象へ切り替えます。

未適用Draftがある場合はTransition Guardへ従います。

## keyboard

- 次のtab
- 前のtab
- tab一覧
- 名前検索
- tabをpin
- tabを閉じる（UIから外すだけ）

をCommand化します。

全てAltから到達できます。

## source切替との違い

physical source tabはProject source切替と同じapplication transitionを使います。

logical View tabはdata contextを壊さず表示対象だけ変えられる場合があります。

どの場合も独自handlerを作らず共通Commandへ収束します。

## missing source

sourceがmissingになってもtab identityを消しません。

degraded / missing表示にし、relink commandへ移れます。

## 名前付きView

同じlogical tableに複数View tabを開けます。

例:

```text
Sales
Sales / 今月
Sales / 未入金
```

CSVデータは共通です。

## 永続化

tabのopen/pin/orderはUI settingとしてprojectへ保存してもよいですが、CSV正本とは無関係です。

bare CSVではsession-only。

## テスト

- tab reorderでsource order不変
- tab closeでsource削除なし
- missing sourceを保持
- named View切替でCSV不変
- Draftありsource切替でTransition Guard
- keyboard-only navigation

この日本語版をワークスペースナビゲーションの正本とします。