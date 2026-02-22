# Pass 004: FFI Thread Affinity And Engine Lifecycle APIs

## Objective

Implement engine lifecycle entry points and enforce the thread-affinity ABI contract at every `ferric_engine_*` boundary.

## Scope

- Engine create/configure/free APIs.
- Canonical thread-check-before-mutation pattern.
- Thread-violation diagnostics and behavior contract.

## Tasks

1. Implement lifecycle APIs (`ferric_engine_new`, `ferric_engine_new_with_config`, `ferric_engine_free`) with null-safe semantics.
2. Apply the canonical two-step borrow pattern on every `ferric_engine_*` entry point (`&` check first, `&mut` only after success).
3. Implement thread-violation behavior contract (debug assert path, release `FERRIC_ERROR_THREAD_VIOLATION` path).
4. Ensure release-path thread-violation diagnostics include both creator/current thread IDs and global error storage.
5. Add tests proving thread checks execute before mutation and that Rust-only thread transfer APIs are not exposed in C.

## Definition Of Done

- Engine lifecycle APIs are usable from C.
- Thread-affinity checks are uniformly enforced with no mutable pre-check paths.
- Violation behavior matches documented debug/release expectations.

## Verification Commands

- `cargo test -p ferric-ffi lifecycle`
- `cargo test -p ferric-ffi thread`
- `cargo check -p ferric-ffi`

## Handoff State

- FFI has a safe, enforceable engine-handle lifecycle and threading baseline.
