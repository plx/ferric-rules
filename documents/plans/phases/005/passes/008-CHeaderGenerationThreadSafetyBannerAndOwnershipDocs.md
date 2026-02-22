# Pass 008: C Header Generation, Thread-Safety Banner, And Ownership Docs

## Objective

Generate and maintain a production-ready C header that matches exported symbols and prominently documents thread/ownership contracts.

## Scope

- Header generation pipeline (`build.rs` + `cbindgen` configuration).
- Required thread-affinity warning block at top of header.
- Ownership/lifetime documentation synchronization.

## Tasks

1. Wire `cbindgen`-based header generation in `ferric-ffi` build flow and commit policy for generated output.
2. Add the required top-of-header thread-safety warning block describing thread affinity and violation behavior.
3. Add ownership/lifetime documentation blocks covering engine-owned strings, caller-owned allocations, and copy semantics.
4. Verify generated declarations/signatures and enum values stay aligned with exported Rust symbols.
5. Add snapshot/diff checks to detect accidental header contract drift.

## Definition Of Done

- `ferric.h` is generated reproducibly and reflects the active exported API.
- Thread-affinity and ownership contracts are prominently documented in the header.
- Header drift is caught by automated checks.

## Verification Commands

- `cargo build -p ferric-ffi`
- `cargo test -p ferric-ffi header`
- `cargo check -p ferric-ffi`

## Handoff State

- Consumers can rely on a synchronized C header as the authoritative external contract.
