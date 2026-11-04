# 表計算ツール機能マップ

## 目的

Rowlyが『CSVを表として開けるツール』で止まらず、日常的な表計算作業を完結できるために必要な機能を整理します。

Excelの内部データモデルは採用しません。

必要な利用体験だけを取り込み、CSV正本・計算分離・非破壊View・共通CommandというRowlyの原則へ変換します。

## 機能階層

### A. 入力・編集

必須:

- cell / range編集
- 行・列の追加 / 削除
- Copy / Paste
- Undo / Redo
- 検索 / 置換
- 複数cell一括編集
- Fill / 連続データ
- Paste Special
- 入力規則
- Codeモード直接編集

Rowlyで既に土台あり:

- cell / range編集
- 行列追加削除
- Copy / Paste
- Undo / Redo
- validation
- Codeモード

追加設計:

- Fill / Series
- Paste Special
- data cleaning / transform

### B. 表示・書式

必須:

- 列幅
- 行高
- 列表示 / 非表示
- 固定行 / 固定列
- Filter / Sort
- Number / Date / Percent等の表示形式
- alignment
- wrapping
- 条件付き書式
- 罫線 / 背景 / 文字強調

Rowlyでは全てCSV外のView設定です。

**罫線・色・表示形式のためにCSV値を変更しません。**

### C. 計算

必須:

- 計算式
- 相対参照
- rangeへの適用
- dependency更新
- エラー表示
- 集計関数

RowlyではCalculation BindingとしてCSV外に持ちます。

追加すべきもの:

- SUM / COUNT / AVERAGE / MIN / MAX等のrange集計
- 条件付き集計
- relative fill / range rule
- lookup系

### D. 集計・分析

必要:

- group by
- subtotal
- pivot / cross tab
- distinct / frequency
- data profile
- Filter / Sort

これらは基本的にDerived / Viewです。

Pivot結果を作るために元CSVを加工しません。

### E. 可視化

必要:

- chart
- conditional formatting
- summary card / KPI

chartはView設定でありCSV正本ではありません。

### F. 出力

必要:

- canonical CSV Save
- Import / Export
- current View export
- clipboard
- print / PDF

正本保存と表示結果の出力を混同しません。

### G. 自動化

必要:

- GUI / keyboard / DSL / CLI / Luauの共通Command
- macro
- command recording
- repeatable transformation

### H. 履歴・安全性

必須:

- Undo / Redo
- external edit coexistence
- conflict inspection
- unsaved guard
- persistent history
- atomic operation
- recovery

## 1.0必須機能

Rowly 1.0で『表計算ツールとして成立』する最低ラインを次とします。

### 編集

- cell / range
- row / column
- Copy / Paste
- Fill / Series
- Search / Replace
- validation
- Edit / View / Code

### 表示

- Filter / Sort
- 列幅 / 行高
- hide / show
- freeze
- display formatting
- conditional formatting

### 計算

- Calculation Binding
- relative reference
- range binding
- SUM / COUNT / AVERAGE / MIN / MAX
- dependency recalc / stale handling
- Materialize

### 分析

- group by / aggregate view
- data profile

### 出力

- CSV
- XLSX Import / Export
- View export
- clipboard
- print / PDF

### 操作性

- finite spreadsheet selection
- keyboard navigation
- Alt KeyTips全到達
- search
- command palette
- undo / redo

### 安全性

- external edit merge
- conflict inspector
- no silent data loss
- save correctness

## 1.0後でもよい機能

- advanced pivot editor
- advanced chart types
- dashboard layout
- comments / annotations
- collaborative editing
- advanced print pagination
- custom theme designer

## Rowlyで禁止するExcel互換

- 数式をCSVセル値へ埋め込む
- merged cellをcanonical dataへ入れる
- display formatでraw値を書き換える
- Filterで行を削除する
- Sortで正本順を勝手に変更する
- Pivot結果を元CSVへ上書きする
- Chart設定をCSVへ埋め込む
- macro recorderでmouse座標を記録する

## 新機能を追加するときの判定

1. 正本データを変える機能か、View / Derivedか。
2. 正本を変えるならUndo単位は何か。
3. 複数sourceならOperationIdでatomicに扱えるか。
4. GUIからAltで到達できるか。
5. DSL / CLI / automationから同じCommandを呼べるか。
6. Projectへ保存する設定か、bare CSV session-onlyか。
7. external change後にidentityを再解決できるか。

この日本語版をRowlyの表計算機能網羅性の正本とします。