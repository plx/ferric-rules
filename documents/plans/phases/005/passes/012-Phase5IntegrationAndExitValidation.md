# Pass 012: Phase 5 Integration And Exit Validation

## Objective

Consolidate all Phase 5 work into a stable baseline that explicitly satisfies the FFI/CLI exit criteria.

## Scope

- End-to-end integration hardening across FFI and CLI.
- Cross-surface diagnostic parity and contract lock-down.
- Full quality-gate and artifact-verification run.

## Tasks

1. Add/update end-to-end fixtures covering C embedding flows (engine lifecycle, load/run, fact mutation, error retrieval, copy-to-buffer retries).
2. Add/update CLI integration fixtures for `run`, `check`, `repl`, and `version`, including exit-code and machine-output assertions.
3. Validate that Phase 4 diagnostics (visibility, ambiguity, module-qualified name failures, generic dispatch/conflict errors) propagate consistently through both FFI and CLI.
4. Run workspace quality gates plus dedicated FFI artifact builds (`ffi-dev`, `ffi-release`) and resolve residual failures/flakiness.
5. Publish concise phase-exit notes mapping delivered behavior to `documents/plans/phases/005/Plan.md` definition-of-done items.

## Definition Of Done

- Phase 5 exit checklist from `documents/plans/phases/005/Plan.md` is satisfied.
- FFI and CLI integration suites pass with stable contract behavior.
- Workspace quality gates and FFI artifact builds are clean and handoff-ready for Phase 6.

## Verification Commands

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo check --workspace --all-targets`
- `cargo build -p ferric-ffi --profile ffi-dev`
- `cargo build -p ferric-ffi --profile ffi-release`

## Handoff State

- FFI and CLI phase is complete and stable.
- Next work naturally begins at Phase 6 polish/compatibility/performance closure.
