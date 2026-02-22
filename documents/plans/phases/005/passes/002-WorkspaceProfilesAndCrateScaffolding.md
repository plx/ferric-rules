# Pass 002: Workspace Profiles And Crate Scaffolding

## Objective

Create the workspace/build baseline for Phase 5 by introducing `ferric-ffi` and `ferric-cli` crates and the FFI profile matrix.

## Scope

- Workspace member wiring for new crates.
- Panic-profile setup for shipped FFI artifacts.
- Minimal compile-ready crate skeletons only.

## Tasks

1. Add `crates/ferric-ffi` and `crates/ferric-cli` to workspace membership and dependency wiring.
2. Add workspace `ffi-dev`/`ffi-release` profiles with `panic = "abort"` while preserving unwind behavior for normal `dev`/`test` profiles.
3. Scaffold `ferric-ffi` structure (`Cargo.toml`, `src/lib.rs`, module placeholders, `build.rs`, `include/`).
4. Scaffold `ferric-cli` structure (`Cargo.toml`, `src/main.rs`, command-module placeholders).
5. Ensure both crates compile in placeholder form and do not regress existing workspace checks.

## Definition Of Done

- New Phase 5 crates exist and are wired into the workspace.
- FFI profile matrix is declared and buildable.
- Workspace remains buildable after scaffolding.

## Verification Commands

- `cargo check -p ferric-ffi`
- `cargo check -p ferric-cli`
- `cargo check --workspace --all-targets`

## Handoff State

- Phase 5 feature passes can build against stable crate/profile scaffolding.
