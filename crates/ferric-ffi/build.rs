/// Thread-safety and ownership documentation prepended to the generated header.
///
/// This constant is also referenced by test code in `src/tests/header.rs`.
pub const HEADER_PREAMBLE: &str = r"/*
 * ferric.h - C API for the Ferric rules engine
 *
 * ============================================================
 * THREAD SAFETY
 * ============================================================
 *
 * Engine handles (FerricEngine*) are bound to the thread that
 * created them. Every ferric_engine_* function validates thread
 * affinity before any state mutation.
 *
 * - Creating thread: all operations succeed normally.
 * - Other threads: operations return FERRIC_ERROR_THREAD_VIOLATION
 *   with a descriptive message in the global error channel.
 * - Exception: ferric_engine_last_error() and
 *   ferric_engine_last_error_copy() skip thread checks
 *   (diagnostic access should always work).
 *
 * The global error functions (ferric_last_error_global, etc.)
 * use thread-local storage and are safe to call from any thread.
 *
 * ============================================================
 * OWNERSHIP AND LIFETIME
 * ============================================================
 *
 * 1. Engine handles: Caller owns the handle returned by
 *    ferric_engine_new(). Must free with ferric_engine_free().
 *
 * 2. Borrowed string pointers: Pointers returned by
 *    ferric_last_error_global() and ferric_engine_last_error()
 *    are valid until the next FFI call that may modify that
 *    error channel. Do NOT free these pointers.
 *
 * 3. Owned string pointers: String fields in FerricValue
 *    (string_ptr for Symbol/String types) are heap-allocated.
 *    Free with ferric_string_free() or ferric_value_free().
 *
 * 4. FerricValue ownership: Values returned through out-params
 *    (e.g., ferric_engine_get_fact_field) are caller-owned.
 *    Free with ferric_value_free() which recursively releases
 *    owned strings and multifield arrays.
 *
 * 5. Multifield arrays: FerricValue.multifield_ptr is a heap-
 *    allocated array. Free with ferric_value_array_free() or
 *    ferric_value_free() (which handles it recursively).
 *
 * 6. External address pointers: FerricValue.external_pointer
 *    is NOT owned by the FFI. Lifetime is caller-managed.
 *
 * 7. Output string pointers: ferric_engine_get_output() returns
 *    a borrowed pointer valid until the next call that writes
 *    to that channel. Do NOT free.
 */";

fn main() {
    let crate_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by Cargo");

    let config = cbindgen::Config::from_file(format!("{crate_dir}/cbindgen.toml"))
        .expect("Failed to read cbindgen.toml");

    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .with_header(HEADER_PREAMBLE)
        .generate()
        .expect("Unable to generate C bindings")
        .write_to_file(format!("{crate_dir}/ferric.h"));
}
