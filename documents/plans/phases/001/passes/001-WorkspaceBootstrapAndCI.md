# Pass 001: Workspace Bootstrap And CI

## Objective

Create a working Rust workspace skeleton and CI gates so subsequent passes can iterate safely.

## Scope

- Workspace layout aligned with Section 4 (at least `ferric`, `ferric-core`, `ferric-parser`, `ferric-runtime`).
- Cargo wiring, basic crate scaffolds, lint/test/format baseline.
- CI pipeline for build hygiene from day one.

## Tasks

1. Create root workspace `Cargo.toml` and crate directories.
2. Add minimal `Cargo.toml` + `src/lib.rs` for each initial crate.
3. Add baseline dependencies used in Phase 1 (`thiserror`, `slotmap`, `smallvec`, etc.) only where needed.
4. Set formatting/linting baseline (rustfmt, clippy policy).
5. Add CI workflow to run:
   - `cargo fmt --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
6. Add one smoke test per critical crate to ensure CI executes real tests.

## Definition Of Done

- `cargo check --workspace` passes.
- CI config exists and is green locally (via equivalent commands).
- Workspace structure is stable enough for implementation passes.

## Verification Commands

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo check --workspace`

## Handoff State

- Clean build.
- Clean tests.
- No temporary scaffolding TODOs that block Pass 002.
