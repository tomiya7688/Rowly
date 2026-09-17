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

## Current implementation

The Rust core currently provides:

- UTF-8 CSV open/save
- Shift_JIS detection and decode on open
- UTF-8 conversion on save
- textual row/cell model
- A1 cell references such as `A1`, `AA10`
- rectangular ranges such as `A1:A8` and `A1:B4`
- atomic range value edits
- row insertion/deletion
- column insertion/deletion with ragged-row preservation
- undo/redo for cell and structural edits
- saved-state-aware dirty tracking
- first-row header lookup with duplicate-header detection
- non-mutating column interpretation checks for `String`, `Integer`, `Decimal`, and `Boolean`
- Japanese-character checks that report matching/non-matching cells by A1 reference
- process-level open/edit/save API
- headless CLI smoke entry point

Column type checks are semantic validation only. They never rewrite canonical CSV cell text.

The GUI, Rowly DSL, Luau integration, Python/Excel bridge, persistent column metadata/type declarations, broader type inference, and visual grouping are intentionally not implemented yet.

For the current architecture and dependency rules, see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).
