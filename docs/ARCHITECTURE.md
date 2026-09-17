# Architecture

## Goal

Rowly is a CSV-first editor. The architecture must make it difficult for presentation, scripting, or integration code to become a second source of truth.

## Dependency direction

```text
UI / Rowly DSL / future Luau / Python adapters
                |
                v
        process/application
                |
                v
              data
```

The `data` module owns canonical table storage and CSV/encoding mechanics.
The `process` module orchestrates user-visible operations such as open, edit, save, addressing, and column interpretation checks.
The `rowly_dsl` module parses/evaluates Rowly commands by calling the process boundary; it does not call CSV codecs directly.
Future UI, Luau, and Python adapters follow the same dependency rule.

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
- scripting syntax or runtime objects
- Excel-specific concepts
- inferred display types

The first row is not inherently special in the canonical model. Header interpretation belongs to a higher layer so CSV structure is not silently redefined.

## Process/application layer

Responsibilities:
- coordinate open/edit/save flows
- expose stable operations to UI and scripting adapters
- track transient session state such as dirty state and source path
- provide A1/range editing and structural row/column operations
- provide explicit header lookup and non-mutating column semantic checks
- translate low-level data errors into application-facing errors

Non-responsibilities:
- CSV byte parsing/encoding details
- rendering
- DSL parsing/evaluation or object lifetime
- Excel implementation details

As the application grows, large operations should be decomposed into small processing modules. Orchestrators should coordinate operations rather than absorb their implementation.

## Rowly DSL adapter

`rowly_dsl` is a user-facing macro language adapter over `process`. Its implementation is split into AST, parser, and runtime responsibilities so language growth does not leak into the data layer.

Current responsibilities:
- parse line-oriented BASIC-style control flow (`If ... Then` / `End If`)
- parse `Let`, top-level `Def ...` / `End Def`, calls, arguments, and `Return`
- parse `Class ...` / `End Class`, `Field`, methods, `New ClassName()`, member reads/writes, and method calls
- evaluate literal, variable, call, object, field, and method expressions plus equality conditions
- maintain a global scope and per-call local scopes; parameters and local `Let` bindings do not overwrite outer variables
- inject `Self` into method scope only while a method is executing
- keep class instances and object identity entirely inside the DSL runtime
- propagate `Return` through nested control flow and require a value when a call is used as an expression
- cap function/method call depth to prevent runaway recursion
- map object-path commands such as `This.Worksheet.Column(...)` and `This.Worksheet.Editor.Cell(...)` to process APIs
- expose execution reports for semantic checks, assignments, calls, and object field state
- preserve top-to-bottom macro execution semantics

Runtime objects are not part of the canonical CSV model. Object aliases may share runtime instance identity, but any effect on CSV must still go through process APIs. CSV cell assignments require text, so object references cannot be silently serialized into canonical data.

Constructors with arguments, inheritance, and richer operators remain deferred language features.

DSL column indices are 1-based because they are user-facing. Process/data indices remain zero-based.

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
- `luau`: embedded general-purpose scripting adapter over the same process API.
- `excel_python`: Python-backed Excel import/export adapter. Excel is an interchange path, not a replacement source of truth for an opened CSV.

These boundaries should be added only when implemented; do not create empty abstraction layers in advance.
