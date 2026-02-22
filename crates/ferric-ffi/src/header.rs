//! C header generation support.
//!
//! The `ferric.h` header is generated at build time by `build.rs` using
//! [`cbindgen`](https://github.com/mozilla/cbindgen). Configuration lives in
//! `cbindgen.toml` at the crate root.
//!
//! The header is committed to version control so that consumers of the library
//! can use it without re-running the build script. When `ferric-ffi` is rebuilt,
//! `build.rs` regenerates `ferric.h` in place. CI can detect drift by checking
//! `git diff --exit-code crates/ferric-ffi/ferric.h` after a build.
//!
//! ## Header structure
//!
//! The generated header contains:
//!
//! 1. The thread-safety and ownership preamble (defined in `build.rs` as
//!    `HEADER_PREAMBLE`).
//! 2. The `FERRIC_H` include guard.
//! 3. The cbindgen autogeneration warning.
//! 4. `FerricError` — C-facing error codes.
//! 5. `FerricValueType` — value-type discriminant enum.
//! 6. `FerricValue` — value struct (all fields).
//! 7. `FerricEngine` — opaque forward declaration (no fields exposed).
//! 8. All `extern "C"` functions, with their Rust doc comments converted to
//!    C-style comments.
