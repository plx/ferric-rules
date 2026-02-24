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
 *
 * 8. Bounds annotations: Pointer parameters and struct fields
 *    carry FERRIC_COUNTED_BY, FERRIC_SIZED_BY, and
 *    FERRIC_NULL_TERMINATED annotations when compiled with
 *    Clang -fbounds-safety. Define FERRIC_NO_BOUNDS_ANNOTATIONS
 *    before including this header to suppress.
 */";

/// Bounds-safety annotation macros injected after the standard includes.
///
/// These macros gate on `__has_feature(bounds_safety)` (Clang with
/// `-fbounds-safety`) and degrade to empty definitions everywhere else.
/// Users can also define `FERRIC_NO_BOUNDS_ANNOTATIONS` to suppress
/// all annotations unconditionally.
const BOUNDS_SAFETY_MACROS: &str = r"
/*
 * ============================================================
 * BOUNDS-SAFETY ANNOTATIONS
 * ============================================================
 *
 * When compiled with a supporting compiler (Clang with
 * -fbounds-safety), pointer parameters, struct fields, and
 * return types carry bounds annotations that enable static
 * and runtime checking.
 *
 * To disable all annotations, define FERRIC_NO_BOUNDS_ANNOTATIONS
 * before including this header.
 */

#ifndef FERRIC_NO_BOUNDS_ANNOTATIONS
  #if defined(__clang__) && defined(__has_feature)
    #if __has_feature(bounds_safety)
      #define FERRIC_COUNTED_BY(N) __counted_by(N)
      #define FERRIC_SIZED_BY(N) __sized_by(N)
      #define FERRIC_NULL_TERMINATED __null_terminated
    #endif
  #endif
  #ifndef FERRIC_COUNTED_BY
    #define FERRIC_COUNTED_BY(N)
    #define FERRIC_SIZED_BY(N)
    #define FERRIC_NULL_TERMINATED
  #endif
#else
  #define FERRIC_COUNTED_BY(N)
  #define FERRIC_SIZED_BY(N)
  #define FERRIC_NULL_TERMINATED
#endif
";

/// Deterministic annotation replacements applied to the cbindgen output.
///
/// Each `(find, replace)` pair must match exactly once in the generated header.
/// If cbindgen's output format changes and a pattern no longer matches, the
/// build will panic with a clear message identifying the stale pattern.
const BOUNDS_ANNOTATIONS: &[(&str, &str)] = &[
    // ── Struct fields ──────────────────────────────────────────────────
    //
    // FerricValue.string_ptr: NUL-terminated string when non-null.
    (
        "char *string_ptr;",
        "char * FERRIC_NULL_TERMINATED string_ptr;",
    ),
    // FerricValue.multifield_ptr: array of multifield_len elements.
    (
        "struct FerricValue *multifield_ptr;",
        "struct FerricValue *multifield_ptr FERRIC_COUNTED_BY(multifield_len);",
    ),
    // ── Return types ───────────────────────────────────────────────────
    //
    // ferric_engine_last_error returns a NUL-terminated string (or null).
    (
        "const char *ferric_engine_last_error(",
        "const char * FERRIC_NULL_TERMINATED ferric_engine_last_error(",
    ),
    // ferric_engine_get_output returns a NUL-terminated string (or null).
    (
        "const char *ferric_engine_get_output(",
        "const char * FERRIC_NULL_TERMINATED ferric_engine_get_output(",
    ),
    // ferric_last_error_global returns a NUL-terminated string (or null).
    (
        "const char *ferric_last_error_global(",
        "const char * FERRIC_NULL_TERMINATED ferric_last_error_global(",
    ),
    // ── NUL-terminated string parameters ───────────────────────────────
    //
    // ferric_engine_load_string: source is NUL-terminated.
    (
        "const char *source);",
        "const char * FERRIC_NULL_TERMINATED source);",
    ),
    // ferric_engine_assert_string: source is NUL-terminated.
    (
        "const char *source,",
        "const char * FERRIC_NULL_TERMINATED source,",
    ),
    // ferric_engine_get_output: channel is NUL-terminated.
    (
        "const char *channel);",
        "const char * FERRIC_NULL_TERMINATED channel);",
    ),
    // ferric_engine_get_global: name is NUL-terminated.
    (
        "const char *name,",
        "const char * FERRIC_NULL_TERMINATED name,",
    ),
    // ferric_string_free: ptr is a NUL-terminated string.
    (
        "ferric_string_free(char *ptr)",
        "ferric_string_free(char * FERRIC_NULL_TERMINATED ptr)",
    ),
    // ── Sized / counted buffer parameters ──────────────────────────────
    //
    // ferric_value_array_free: arr is an array of len FerricValues.
    (
        "ferric_value_array_free(struct FerricValue *arr, uintptr_t len)",
        "ferric_value_array_free(struct FerricValue *arr FERRIC_COUNTED_BY(len), uintptr_t len)",
    ),
    // ferric_last_error_global_copy: buf is a byte buffer of buf_len bytes.
    (
        "ferric_last_error_global_copy(char *buf, uintptr_t buf_len,",
        "ferric_last_error_global_copy(char *buf FERRIC_SIZED_BY(buf_len), uintptr_t buf_len,",
    ),
    // ferric_engine_last_error_copy: buf is a byte buffer of buf_len bytes.
    // (multi-line signature — pattern spans the line break)
    (
        "ferric_engine_last_error_copy(const struct FerricEngine *engine,\n                                               char *buf,",
        "ferric_engine_last_error_copy(const struct FerricEngine *engine,\n                                               char *buf FERRIC_SIZED_BY(buf_len),",
    ),
    // ferric_engine_action_diagnostic_copy: buf is a byte buffer of buf_len bytes.
    // (multi-line signature — pattern spans the line break)
    (
        "uintptr_t index,\n                                                      char *buf,",
        "uintptr_t index,\n                                                      char *buf FERRIC_SIZED_BY(buf_len),",
    ),
];

fn main() {
    let crate_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by Cargo");

    let config = cbindgen::Config::from_file(format!("{crate_dir}/cbindgen.toml"))
        .expect("Failed to read cbindgen.toml");

    // Generate the header into memory so we can post-process it.
    let bindings = cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .with_header(HEADER_PREAMBLE)
        .generate()
        .expect("Unable to generate C bindings");

    let mut buf = Vec::new();
    bindings.write(&mut buf);
    let mut header = String::from_utf8(buf).expect("cbindgen output was not valid UTF-8");

    // Inject bounds-safety macro definitions after the standard includes.
    let inject_marker = "#include <stdlib.h>";
    let inject_pos = header
        .find(inject_marker)
        .expect("Could not find #include <stdlib.h> in generated header")
        + inject_marker.len();
    header.insert_str(inject_pos, BOUNDS_SAFETY_MACROS);

    // Apply bounds-safety annotations to struct fields, function parameters,
    // and return types. Each pattern must match exactly once; if cbindgen's
    // output drifts, the build fails loudly rather than silently dropping
    // an annotation.
    for (find, replace) in BOUNDS_ANNOTATIONS {
        let count = header.matches(find).count();
        assert_eq!(
            count, 1,
            "bounds-safety annotation: expected exactly 1 match, found {count} for pattern:\n  {find}"
        );
        header = header.replacen(find, replace, 1);
    }

    std::fs::write(format!("{crate_dir}/ferric.h"), header).expect("Failed to write ferric.h");
}
