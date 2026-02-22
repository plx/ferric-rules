# Pass 006: External Surface Compatibility CLI JSON Contract Suite

## Objective

Lock machine-readable CLI diagnostics behavior and top-level CLI contract semantics for non-interactive consumers.

## Scope

- `ferric run --json` and `ferric check --json` diagnostics contracts.
- Field-preservation and additive-evolution expectations for JSON output.
- Exit-code and stream behavior in machine mode.

## Tasks

1. Add/expand CLI integration tests for `run --json` and `check --json` success/failure paths.
2. Define and enforce stable JSON shape assertions (top-level fields and per-diagnostic fields with source locations when available).
3. Add regression assertions for additive-evolution policy (new fields allowed, existing documented fields not repurposed).
4. Validate machine-mode exit codes and stream routing (`stdout` vs `stderr`) remain deterministic.
5. Add representative CI-oriented fixtures for parser/compile/runtime/action diagnostic emission in JSON mode.

## Definition Of Done

- CLI JSON compatibility contracts are explicit, tested, and stable.
- Machine-mode behavior is regression-protected for CI/tooling consumers.

## Verification Commands

- `cargo test -p ferric-cli --test cli_integration`
- `cargo check -p ferric-cli`

## Handoff State

- CLI external contract is locked alongside FFI for Phase 6 completion.
