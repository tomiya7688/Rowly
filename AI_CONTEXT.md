# AI Context

Small routing index for AI-assisted development. Do not duplicate detailed specifications here.

## Project
- Name: Rowly
- Purpose: CSV-first table viewer/editor. CSV remains the canonical data source.
- Main language/runtime: Rust.
- Planned adapters: Rowly DSL and Luau scripting; Python for Excel I/O.

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
Bootstrap implementation only:
- CSV text table model
- UTF-8 / Shift_JIS open path
- UTF-8 save path
- headless CLI smoke entry point

Not implemented yet:
- GUI
- Rowly DSL
- Luau
- Python/Excel bridge
- type inference/column metadata
- visual grouping

## Context Priority
- P0: current task and invariants
- P1: target source and tests
- P2: direct dependencies
- P3: architecture/reference docs
- P4: history and auxiliary material
