# Architecture

## Goal

Rowly is a CSV-first editor. The architecture must make it difficult for presentation, scripting, or integration code to become a second source of truth.

## Dependency direction

```text
future UI / DSL / Luau / Python adapters
                |
                v
        process/application
                |
                v
              data
```

The `data` module owns canonical table storage and CSV/encoding mechanics.
The `process` module orchestrates user-visible operations such as open, edit, and save.
Future UI and scripting adapters call the process boundary; they do not call CSV codecs directly.

This borrows the responsibility and dependency-direction principles from UPD Commander Base Design without mechanically reproducing its class-oriented Commander/Messenger structure in Rust. Rust modules, visibility, and narrow APIs are used to enforce the same intent with less ceremony.

## Data layer

Responsibilities:
- store CSV rows/cells as text
- decode supported source encodings into internal UTF-8 text
- parse CSV records
- write canonical CSV output

Non-responsibilities:
- UI state
- visual grouping
- scripting syntax
- Excel-specific concepts
- inferred display types

The first row is not inherently special in the canonical model. Header interpretation belongs to a higher layer so CSV structure is not silently redefined.

## Process/application layer

Responsibilities:
- coordinate open/edit/save flows
- expose stable operations to UI and scripting adapters
- track transient session state such as dirty state and source path
- translate low-level data errors into application-facing errors

Non-responsibilities:
- CSV byte parsing/encoding details
- rendering
- DSL parsing/evaluation
- Excel implementation details

As the application grows, large operations should be decomposed into small processing modules. Orchestrators should coordinate operations rather than absorb their implementation.

## Encoding policy

- Internal text representation is UTF-8 Rust `String` data.
- UTF-8 input is accepted, including UTF-8 BOM.
- Shift_JIS is detected and decoded to UTF-8 on open.
- Unsupported detected encodings fail explicitly rather than being silently mis-decoded.
- Save output is UTF-8.

UI policy around notifying the user about Shift_JIS -> UTF-8 conversion is intentionally deferred until the GUI exists.

## CSV fidelity

Rowly preserves table meaning as rows and textual cells. Saving may normalize byte-level CSV representation (for example quoting or line endings) while preserving parsed records and values.

Exact byte-for-byte round-tripping is not an architectural requirement. If future requirements make blank-line or dialect fidelity significant, that must be handled explicitly in the data layer rather than hidden in UI metadata.

## Future boundaries

- `ui`: concrete viewer/editor toolkit and rendering; depends on process only.
- `rowly_dsl`: parser/evaluator that maps commands to process operations.
- `luau`: embedded general-purpose scripting adapter over the same process API.
- `excel_python`: Python-backed Excel import/export adapter. Excel is an interchange path, not a replacement source of truth for an opened CSV.

These boundaries should be added only when implemented; do not create empty abstraction layers in advance.
