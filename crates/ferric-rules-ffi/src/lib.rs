//! # Ferric FFI
//!
//! C-ABI foreign function interface for the Ferric rules engine.
//!
//! ## Phase 5 Baseline Assumptions
//!
//! This crate provides a stable C-callable API surface over the Ferric runtime.
//! The following invariants from Phase 4 are preserved:
//!
//! - **Diagnostic parity**: All runtime diagnostics (parse errors, compile errors,
//!   module visibility/ambiguity failures, generic dispatch/conflict diagnostics)
//!   are surfaced through FFI without reinterpretation or loss of source context.
//!
//! - **Error-channel synchronization**: Any failed call involving a validated
//!   raw-engine handle publishes the same current message to that engine's
//!   snapshot and the calling thread's global fallback. Failures before handle
//!   validation can update only the global channel.
//!
//! - **Thread affinity**: A raw engine handle is bound to its creating thread.
//!   Runtime operations validate affinity before accessing the engine. The
//!   synchronized `ferric_engine_last_error_copy` accessor may run concurrently
//!   from any thread; the borrowed `ferric_engine_last_error` accessor also
//!   skips affinity but requires external serialization for pointer use. The
//!   destruction-only `ferric_engine_free_unchecked` escape hatch also skips
//!   affinity and must not overlap any access to the engine. The
//!   internal `unsafe fn move_to_current_thread` is deliberately NOT exposed in
//!   the C API.
//!
//! - **Ownership conventions**: Callers own handles returned by `_new` functions and
//!   must free them with corresponding `_free` functions. The borrowed raw-engine
//!   error pointer remains valid until the next borrowed read on that engine or
//!   engine destruction; concurrent consumers should use the copy API. Borrowed
//!   output snapshots are stored per engine and channel, cannot be invalidated by
//!   another engine, and are reclaimed at engine destruction. Hosts should prefer
//!   `ferric_engine_get_output_copy` for caller-owned output storage.
//!
//! - **Embedded-NUL policy**: Legacy NUL-terminated inputs end at their first
//!   NUL. Hosts converting length-bearing strings must reject embedded NUL
//!   before calling those entry points; the checked value constructors do this
//!   explicitly. Legacy `FerricValue` and borrowed-output egress reject
//!   unrepresentable content instead of returning empty/truncated strings.
//!   Length-reporting copy and serialization APIs preserve exact bytes.
//!
//! - **Logical-run continuation**: `ferric_engine_run_ex` starts a fresh logical
//!   run. After it returns `LimitReached`, `ferric_engine_continue_run_ex` runs
//!   another bounded chunk without clearing a pending halt request or action
//!   diagnostics. Chunk counts are per-call; terminal reasons close
//!   continuation eligibility. Host cancellation remains a distinct,
//!   host-owned outcome.
//!
//! - **Panic containment**: Every C export is generated around a non-extern
//!   implementation and catches ordinary Rust panics before they reach the ABI.
//!   `ffi-dev` and `ffi-release` retain unwind support for that purpose. A
//!   contained panic records a payload-independent internal-error diagnostic
//!   and returns the documented sentinel for its return category. Allocator
//!   abort/OOM and other non-unwinding process termination remain outside the
//!   guarantee.
//!
//! ## Build Instructions
//!
//! Ferric FFI ships with two dedicated profiles for C-ABI-safe builds:
//!
//! - **ffi-dev**: Development builds with unwind-capable containment and debug info.
//!   ```sh
//!   cargo build -p ferric-rules-ffi --profile ffi-dev
//!   ```
//!
//! - **ffi-release**: Optimized builds with unwind-capable containment.
//!   ```sh
//!   cargo build -p ferric-rules-ffi --profile ffi-release
//!   ```
//!
//! ### Artifacts
//!
//! | Platform | Dynamic Library       | Static Library   |
//! |----------|-----------------------|------------------|
//! | macOS    | `libferric_rules_ffi.dylib` | `libferric_rules_ffi.a` |
//! | Linux    | `libferric_rules_ffi.so`    | `libferric_rules_ffi.a` |
//! | Windows  | `ferric_rules_ffi.dll`      | `ferric_rules_ffi.lib`  |
//!
//! Artifacts are placed in `target/<profile>/`.
//!
//! ### Panic Policy
//!
//! Both FFI profiles use `panic = "unwind"` so generated export wrappers can
//! catch ordinary Rust panics before they reach the C ABI boundary. Panics are
//! converted to stable sentinels and internal-error diagnostics. Non-unwinding
//! termination such as allocator abort/OOM remains outside the guarantee.
//!
//! ## Module Organization
//!
//! - `error` — `FerricError` enum, error mapping, thread-local/per-engine storage
//! - `engine` — Engine lifecycle, execution, and fact manipulation APIs
//! - `types` — C-facing value types and conversion helpers
//! - `header` — C header generation support

pub mod engine;
pub mod error;
pub mod header;
pub mod pinned;
pub mod types;

mod boundary;

#[cfg(test)]
mod tests;
