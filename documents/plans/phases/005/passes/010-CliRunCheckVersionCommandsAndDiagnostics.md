# Pass 010: CLI `run`/`check`/`version` Commands And Diagnostics

## Objective

Implement non-interactive CLI command behavior with stable exit codes and machine-consumable diagnostics.

## Scope

- Commands: `run`, `check`, `version`.
- Exit-code contracts and output-channel conventions.
- Diagnostic formatting for parser/compile/runtime failures.

## Tasks

1. Implement CLI command dispatch for `run`, `check`, and `version` with documented argument behavior.
2. Implement `run` pipeline (`load -> execute`) and `check` pipeline (`load/validate only`) with correct exit semantics.
3. Surface source-located diagnostics in CLI output while preserving runtime-authored diagnostic meaning.
4. Add machine-friendly output mode(s) for CI automation and parseable failure reporting.
5. Add integration tests covering success/failure exit codes, missing file errors, and Phase 4 diagnostic parity scenarios.

## Definition Of Done

- `run`, `check`, and `version` commands are functional and test-backed.
- Exit behavior is deterministic and matches documented contracts.
- CLI diagnostics preserve source context and semantic fidelity.

## Verification Commands

- `cargo test -p ferric-cli run`
- `cargo test -p ferric-cli check`
- `cargo check -p ferric-cli`

## Handoff State

- Ferric CLI is usable for batch execution and validation workflows.
