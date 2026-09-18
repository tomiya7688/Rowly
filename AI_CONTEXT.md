# AI Context

Small routing index for AI-assisted development. Do not duplicate detailed specifications here.

## Project
- Name: Rowly
- Purpose: CSV-first table viewer/editor. CSV remains the canonical data source.
- Main language/runtime: Rust.
- Adapters: Rowly DSL and Luau scripting are implemented in Rust; Python is planned for Excel I/O.

## Source of Truth
- Product invariants: `README.md`
- Architecture and dependency rules: `docs/ARCHITECTURE.md`
- Implementation truth: `src/` and matching tests
- Task intent: the current issue/conversation takes priority over speculative future work

## Read First
1. Current task / issue
2. `README.md`
3. `docs/ARCHITECTURE.md`
4. Only the target source module and its tests

## Ignore Normally
- `target/`
- generated artifacts and caches
- large logs
- unrelated Git history, issues, docs, or future feature sketches

## Current Task Rules
- Stop exploration when Goal / Required / Acceptance are sufficient.
- Search first, read second.
- Prefer target source -> matching tests -> detailed docs.
- Do not mix unrelated refactors into the current change.
- Summaries are routing aids, not substitutes for source/tests.

## Important Constraints
- CSV is the source of truth; Rowly-only metadata must never be required to recover table data.
- Never mutate CSV structure merely for presentation.
- No merged cells in the data model.
- Core cell storage is textual CSV data. Type inference/formatting belongs above the canonical data layer unless the user explicitly edits data.
- Internal text is UTF-8. Shift_JIS input may be decoded on open; saves are UTF-8.
- UI, DSL, Luau, and Python adapters must enter through the process/application boundary rather than reaching into CSV I/O internals.
- A1/range addressing is process-layer behavior; the canonical data layer remains zero-based textual rows/cells.
- Multi-cell edits must validate before mutation so a failed edit cannot partially modify the CSV table.
- Structural edits are explicit data edits. Ragged rows must not be silently rectangularized merely to simplify column operations.
- Undo/redo for structural edits must restore exact removed row/cell values, not reconstructed approximations.
- Header lookup and column semantic checks live above the canonical data layer. The first row is only treated as a header when a process-layer header API is explicitly used.
- Column type checks are non-mutating interpretation/validation. `String`, numeric, and boolean checks must not rewrite canonical CSV text.
- Duplicate header names are ambiguous for singular lookup and must be reported instead of silently selecting one.
- Rowly DSL is an adapter over `process`; it must not call `data` or CSV codecs directly.
- The DSL is top-to-bottom macro execution. Extend language features through AST/parser/runtime boundaries rather than bypassing them.
- DSL column numbers are 1-based user-facing indices; process/data indices remain zero-based.
- Function and method calls use local scopes, may read outer/global variables, and must not leak local bindings outward.
- Calls used as expressions require an explicit return value.
- Runtime call depth is capped to prevent runaway recursion.
- Class instances are runtime-only DSL objects. They must not become canonical table state or bypass process-layer edits.
- Object aliases share instance identity. `Self` is injected only while a method executes.
- CSV cell edits require textual values; object references cannot be written directly into canonical CSV cells.
- DSL arithmetic is typed: Integer/Decimal only, with explicit promotion, division-by-zero errors, and no implicit string coercion.
- DSL predicate builtins return Boolean values and Boolean expressions may be used directly as `If` conditions.
- `CellValue("A1")` reads current CSV text through the process boundary; missing cells and invalid references must fail explicitly.

## Validation
Use the smallest sufficient validation for the change:
- Format: `cargo fmt --all -- --check`
- Targeted/full tests while the suite is small: `cargo test --all-targets --all-features`
- Lints: `cargo clippy --all-targets --all-features -- -D warnings`
- GUI work: prefer headless process/data tests first; add visual confirmation only when acceptance requires it.
- Report unverified areas explicitly when the required runtime/toolchain is unavailable.

## Remote Delta
Multiple AI sessions may work on the repository.
Before writing remote changes, compare the current base branch head with the task base. If it moved, inspect changed files/diff scope before editing; do not overwrite unrelated remote work.

## Current State
Implemented:
- CSV text table model
- UTF-8 / Shift_JIS open path
- UTF-8 save path
- A1 cell and rectangular range references
- atomic range value edits
- row insertion/deletion
- column insertion/deletion with ragged-row preservation
- undo/redo history for cell and structural edits with saved-state-aware dirty tracking
- process-layer first-row header lookup with duplicate detection
- non-mutating `String` / `Integer` / `Decimal` / `Boolean` column validation
- Japanese-character column checks with A1 result references
- Rowly DSL AST/parser/runtime for `If`, variables, functions, calls, return, classes, objects, fields, methods, arithmetic expressions, column checks, and range value assignment
- typed Integer/Decimal arithmetic with precedence, unary minus, parentheses, and explicit arithmetic errors
- value predicates for string matching, Japanese detection, and Integer/Decimal/Boolean interpretation
- expression-level CSV cell reads through `CellValue(...)`
- headless CLI smoke entry point

Not implemented yet:
- GUI
- Rowly DSL inheritance/constructors
- Python/Excel bridge
- persistent column metadata/type declarations
- broader type inference
- visual grouping

## Context Priority
- P0: current task and invariants
- P1: target source and tests
- P2: direct dependencies
- P3: architecture/reference docs
- P4: history and auxiliary material
