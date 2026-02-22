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
//! - **Thread affinity**: An engine handle is bound to its creating thread.
//!   Every `ferric_engine_*` entry point validates thread affinity before any
//!   mutable borrow or state mutation. The internal `unsafe fn move_to_current_thread`
//!   is deliberately NOT exposed in the C API.
//!
//! - **Ownership conventions**: Callers own handles returned by `_new` functions and
//!   must free them with corresponding `_free` functions. String pointers returned by
//!   error-retrieval APIs are valid only until the next call that may modify the
//!   error channel.
//!
//! - **Panic policy**: FFI builds use `panic = "abort"` profiles (`ffi-dev`,
//!   `ffi-release`) so that Rust panics never unwind across the C ABI boundary.
//!
//! ## Build Instructions
//!
//! Ferric FFI ships with two dedicated profiles for C-ABI-safe builds:
//!
//! - **ffi-dev**: Development builds with `panic = "abort"` and debug info.
//!   ```sh
//!   cargo build -p ferric-ffi --profile ffi-dev
//!   ```
//!
//! - **ffi-release**: Release builds with `panic = "abort"` and optimizations.
//!   ```sh
//!   cargo build -p ferric-ffi --profile ffi-release
//!   ```
//!
//! ### Artifacts
//!
//! | Platform | Dynamic Library       | Static Library   |
//! |----------|-----------------------|------------------|
//! | macOS    | `libferric_ffi.dylib` | `libferric_ffi.a` |
//! | Linux    | `libferric_ffi.so`    | `libferric_ffi.a` |
//! | Windows  | `ferric_ffi.dll`      | `ferric_ffi.lib`  |
//!
//! Artifacts are placed in `target/<profile>/`.
//!
//! ### Panic Policy
//!
//! Both FFI profiles use `panic = "abort"` to prevent Rust panics from
//! unwinding across the C ABI boundary. The default `dev`/`release` profiles
//! retain normal unwind semantics for ergonomic development and testing.
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
pub mod types;

#[cfg(test)]
mod tests;
