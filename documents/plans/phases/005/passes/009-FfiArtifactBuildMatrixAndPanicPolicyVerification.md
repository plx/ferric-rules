# Pass 009: FFI Artifact Build Matrix And Panic Policy Verification

## Objective

Finalize FFI build outputs and validate the no-unwind-across-FFI policy through profile-driven artifact tests.

## Scope

- `cdylib`/`staticlib` artifact configuration.
- `ffi-dev`/`ffi-release` build paths.
- Subprocess verification of abort-on-panic behavior.

## Tasks

1. Ensure `ferric-ffi` crate types and artifact settings produce expected dynamic/static outputs per platform.
2. Validate `ffi-dev` and `ffi-release` profile builds succeed and are documented for embedders.
3. Add subprocess-based tests that prove panic abort behavior for shipped FFI artifacts.
4. Confirm default Rust development/test workflows retain unwind ergonomics.
5. Publish concise build/packaging notes for Linux/macOS/Windows artifact expectations.

## Definition Of Done

- FFI artifacts build correctly under dedicated shipping profiles.
- Panic policy is validated by tests targeting produced artifacts.
- Build instructions for artifact consumers are explicit and current.

## Verification Commands

- `cargo build -p ferric-ffi --profile ffi-dev`
- `cargo build -p ferric-ffi --profile ffi-release`
- `cargo test -p ferric-ffi subprocess`

## Handoff State

- FFI build and panic-policy contracts are production-ready.
