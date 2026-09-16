# Rowly

Rowly is a table viewer and editor built around a simple principle:

> **CSV is the source of truth.**

## Core philosophy

### CSV is the source of truth

Rowly does not import CSV into a proprietary document model and make that model authoritative.
The CSV itself is always the canonical data.

Features that exist only for presentation or convenience must not change the meaning or structure of the underlying CSV.
Rowly-specific metadata, if introduced, is auxiliary and must never be required to recover the actual table data.

### No merged cells

Rowly does not have merged cells.
A table remains a flat collection of rows and columns, matching the structure of CSV.

### Group repeated values visually, not structurally

When consecutive rows contain the same value in a column, Rowly may display those values as a single visual group.

For example, data like this:

```csv
Department,Name
Sales,Tanaka
Sales,Sato
Sales,Suzuki
Development,Yamada
Development,Takahashi
```

may visually group the consecutive `Sales` and `Development` values.
However, the underlying CSV remains completely unchanged: every row still contains its own value.

> **Never change the data structure just to improve presentation.**
