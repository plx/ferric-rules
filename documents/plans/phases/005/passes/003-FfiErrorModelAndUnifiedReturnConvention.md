# Pass 003: FFI Error Model And Unified Return Convention

## Objective

Implement the base C-facing error model and return-code conventions shared by all FFI entry points.

## Scope

- `FerricError` ABI type and code mapping.
- Thread-local and per-engine error storage baseline.
- Error retrieval/clear APIs and mapping helpers.

## Tasks

1. Define a stable `#[repr(C)]` `FerricError` enum and centralized Rust-to-FFI error mapping.
2. Implement thread-local global error storage APIs (`ferric_last_error_global`, `ferric_clear_error_global`).
3. Add per-engine last-error storage and APIs (`ferric_engine_last_error`, `ferric_engine_clear_error`).
4. Enforce the unified return convention for fallible APIs (`FerricError` + out-parameters).
5. Add tests for null-pointer handling, error-channel separation (global vs per-engine), and message-lifetime behavior.

## Definition Of Done

- Core FFI error channels are operational and deterministic.
- Fallible API return-shape conventions are codified and test-backed.
- Error diagnostics remain available through the appropriate channel.

## Verification Commands

- `cargo test -p ferric-ffi error`
- `cargo test -p ferric-ffi api_errors`
- `cargo check -p ferric-ffi`

## Handoff State

- FFI entry points can build on a consistent error/reporting foundation.
