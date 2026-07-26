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
 *
 * ============================================================
 * PINNED EXECUTION (ferric_pinned_*)
 * ============================================================
 *
 * FerricPinnedEngine owns a dedicated Rust worker thread plus
 * one engine. The handle is safe to use from any thread; calls
 * are serialized through a bounded FIFO queue on the worker.
 *
 * - Sync entry points (ferric_pinned_engine_load_string, _reset,
 *   _run, _serialize_as) block the caller until the worker
 *   completes the operation, then write outputs into caller-
 *   provided pointers.
 *
 * - Async entry points (ferric_pinned_engine_run_async,
 *   _load_string_async) return immediately on successful
 *   submission and later invoke the supplied
 *   FerricPinnedCompletionFn with an owned FerricPinnedResult
 *   carrying the echoed request_id.
 *
 * The async completion callback runs ON THE WORKER THREAD.
 * It must be transport-only: resume a continuation, signal an
 * event, post to an actor / event loop. It must NOT call back
 * into the same FerricPinnedEngine synchronously, perform long
 * work, or block, and must not unwind across the FFI boundary.
 * The owned FerricPinnedResult outlives the callback; the caller
 * is responsible for ferric_pinned_result_free.
 *
 * Halt: ferric_pinned_engine_halt() requests cancellation of
 * the active run, which checks between bounded chunks of rule
 * firings (cooperative cancellation, not hard preemption). If
 * no run is active, halt has no effect and does not latch onto
 * queued or future runs.
 *
 * ferric_pinned_engine_cancel_request() targets one async
 * request by ID. Capacity waiters wake without being admitted,
 * admitted pending requests are canceled before dispatch, and
 * an active run is interrupted cooperatively at a chunk boundary.
 * A successful cancel does not guarantee that a concurrent
 * submission succeeds; any failed submission fires no completion.
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

/// Compile-time lock on ABI discriminant widths and numeric values.
///
/// Appended to the generated header immediately before the closing include
/// guard. Discriminants cross the ABI as fixed-width 32-bit integers with
/// the documented numeric values below; these assertions fail a consumer's
/// build if the header ever drifts from that contract.
const STATIC_ASSERTIONS: &str = r#"
/*
 * ============================================================
 * ABI STATIC ASSERTIONS
 * ============================================================
 *
 * Caller-populated discriminants cross this ABI as fixed-width
 * 32-bit integers (never as C enum objects), and every accepting
 * API validates them before interpretation: unknown values are
 * rejected with FERRIC_ERROR_INVALID_ARGUMENT. The assertions
 * below lock the field widths and the documented numeric values
 * at compile time for every consumer of this header.
 */

#if defined(__cplusplus) && __cplusplus >= 201103L
  #define FERRIC_STATIC_ASSERT(COND, MSG) static_assert(COND, MSG)
#elif defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
  #define FERRIC_STATIC_ASSERT(COND, MSG) _Static_assert(COND, MSG)
#else
  #define FERRIC_STATIC_ASSERT_CAT2(A, B) A##B
  #define FERRIC_STATIC_ASSERT_CAT(A, B) FERRIC_STATIC_ASSERT_CAT2(A, B)
  #define FERRIC_STATIC_ASSERT(COND, MSG) \
      typedef char FERRIC_STATIC_ASSERT_CAT(ferric_static_assert_, __LINE__)[(COND) ? 1 : -1]
#endif

/* Discriminant field widths (all fixed at 32 bits). */
FERRIC_STATIC_ASSERT(sizeof(((FerricValue *)0)->value_type) == 4,
                     "FerricValue.value_type must be a 32-bit integer");
FERRIC_STATIC_ASSERT(sizeof(((FerricValue *)0)->external_type_id) == 4,
                     "FerricValue.external_type_id must be a 32-bit integer");
FERRIC_STATIC_ASSERT(sizeof(((FerricConfig *)0)->string_encoding) == 4,
                     "FerricConfig.string_encoding must be a 32-bit integer");
FERRIC_STATIC_ASSERT(sizeof(((FerricConfig *)0)->strategy) == 4,
                     "FerricConfig.strategy must be a 32-bit integer");
FERRIC_STATIC_ASSERT(sizeof(((FerricPinnedEngineOptions *)0)->autorelease_policy) == 4,
                     "FerricPinnedEngineOptions.autorelease_policy must be a 32-bit integer");

/* C enum object widths for every enum crossing the ABI (as return values
 * or out-parameters). Rust emits these as 32-bit values; a consumer
 * compiled with -fshort-enums (or an equivalent packed-enum ABI) fails
 * these assertions instead of silently miscompiling. */
FERRIC_STATIC_ASSERT(sizeof(enum FerricError) == 4, "enum FerricError must be 32 bits");
FERRIC_STATIC_ASSERT(sizeof(enum FerricValueType) == 4, "enum FerricValueType must be 32 bits");
FERRIC_STATIC_ASSERT(sizeof(enum FerricStringEncoding) == 4,
                     "enum FerricStringEncoding must be 32 bits");
FERRIC_STATIC_ASSERT(sizeof(enum FerricConflictStrategy) == 4,
                     "enum FerricConflictStrategy must be 32 bits");
FERRIC_STATIC_ASSERT(sizeof(enum FerricFactType) == 4, "enum FerricFactType must be 32 bits");
FERRIC_STATIC_ASSERT(sizeof(enum FerricHaltReason) == 4, "enum FerricHaltReason must be 32 bits");
FERRIC_STATIC_ASSERT(sizeof(enum FerricPinnedAutoreleasePolicy) == 4,
                     "enum FerricPinnedAutoreleasePolicy must be 32 bits");

/* FerricValueType: stable numeric values. */
FERRIC_STATIC_ASSERT(FERRIC_VALUE_TYPE_VOID == 0, "FERRIC_VALUE_TYPE_VOID must be 0");
FERRIC_STATIC_ASSERT(FERRIC_VALUE_TYPE_INTEGER == 1, "FERRIC_VALUE_TYPE_INTEGER must be 1");
FERRIC_STATIC_ASSERT(FERRIC_VALUE_TYPE_FLOAT == 2, "FERRIC_VALUE_TYPE_FLOAT must be 2");
FERRIC_STATIC_ASSERT(FERRIC_VALUE_TYPE_SYMBOL == 3, "FERRIC_VALUE_TYPE_SYMBOL must be 3");
FERRIC_STATIC_ASSERT(FERRIC_VALUE_TYPE_STRING == 4, "FERRIC_VALUE_TYPE_STRING must be 4");
FERRIC_STATIC_ASSERT(FERRIC_VALUE_TYPE_MULTIFIELD == 5, "FERRIC_VALUE_TYPE_MULTIFIELD must be 5");
FERRIC_STATIC_ASSERT(FERRIC_VALUE_TYPE_EXTERNAL_ADDRESS == 6,
                     "FERRIC_VALUE_TYPE_EXTERNAL_ADDRESS must be 6");

/* FerricStringEncoding: stable numeric values. */
FERRIC_STATIC_ASSERT(FERRIC_STRING_ENCODING_ASCII == 0, "FERRIC_STRING_ENCODING_ASCII must be 0");
FERRIC_STATIC_ASSERT(FERRIC_STRING_ENCODING_UTF8 == 1, "FERRIC_STRING_ENCODING_UTF8 must be 1");
FERRIC_STATIC_ASSERT(FERRIC_STRING_ENCODING_ASCII_SYMBOLS_UTF8_STRINGS == 2,
                     "FERRIC_STRING_ENCODING_ASCII_SYMBOLS_UTF8_STRINGS must be 2");

/* FerricConflictStrategy: stable numeric values. */
FERRIC_STATIC_ASSERT(FERRIC_CONFLICT_STRATEGY_DEPTH == 0, "FERRIC_CONFLICT_STRATEGY_DEPTH must be 0");
FERRIC_STATIC_ASSERT(FERRIC_CONFLICT_STRATEGY_BREADTH == 1,
                     "FERRIC_CONFLICT_STRATEGY_BREADTH must be 1");
FERRIC_STATIC_ASSERT(FERRIC_CONFLICT_STRATEGY_LEX == 2, "FERRIC_CONFLICT_STRATEGY_LEX must be 2");
FERRIC_STATIC_ASSERT(FERRIC_CONFLICT_STRATEGY_MEA == 3, "FERRIC_CONFLICT_STRATEGY_MEA must be 3");

/* FerricFactType: stable numeric values. */
FERRIC_STATIC_ASSERT(FERRIC_FACT_TYPE_ORDERED == 0, "FERRIC_FACT_TYPE_ORDERED must be 0");
FERRIC_STATIC_ASSERT(FERRIC_FACT_TYPE_TEMPLATE == 1, "FERRIC_FACT_TYPE_TEMPLATE must be 1");

/* FerricHaltReason: stable numeric values. */
FERRIC_STATIC_ASSERT(FERRIC_HALT_REASON_AGENDA_EMPTY == 0,
                     "FERRIC_HALT_REASON_AGENDA_EMPTY must be 0");
FERRIC_STATIC_ASSERT(FERRIC_HALT_REASON_LIMIT_REACHED == 1,
                     "FERRIC_HALT_REASON_LIMIT_REACHED must be 1");
FERRIC_STATIC_ASSERT(FERRIC_HALT_REASON_HALT_REQUESTED == 2,
                     "FERRIC_HALT_REASON_HALT_REQUESTED must be 2");

/* FerricPinnedAutoreleasePolicy: stable numeric values. */
FERRIC_STATIC_ASSERT(FERRIC_PINNED_AUTORELEASE_POLICY_NONE == 0,
                     "FERRIC_PINNED_AUTORELEASE_POLICY_NONE must be 0");
FERRIC_STATIC_ASSERT(FERRIC_PINNED_AUTORELEASE_POLICY_PER_ITEM == 1,
                     "FERRIC_PINNED_AUTORELEASE_POLICY_PER_ITEM must be 1");
FERRIC_STATIC_ASSERT(FERRIC_PINNED_AUTORELEASE_POLICY_PER_BATCH == 2,
                     "FERRIC_PINNED_AUTORELEASE_POLICY_PER_BATCH must be 2");

/* FerricError: stable numeric values. */
FERRIC_STATIC_ASSERT(FERRIC_ERROR_OK == 0, "FERRIC_ERROR_OK must be 0");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_NULL_POINTER == 1, "FERRIC_ERROR_NULL_POINTER must be 1");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_THREAD_VIOLATION == 2, "FERRIC_ERROR_THREAD_VIOLATION must be 2");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_NOT_FOUND == 3, "FERRIC_ERROR_NOT_FOUND must be 3");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_PARSE_ERROR == 4, "FERRIC_ERROR_PARSE_ERROR must be 4");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_COMPILE_ERROR == 5, "FERRIC_ERROR_COMPILE_ERROR must be 5");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_RUNTIME_ERROR == 6, "FERRIC_ERROR_RUNTIME_ERROR must be 6");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_IO_ERROR == 7, "FERRIC_ERROR_IO_ERROR must be 7");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_BUFFER_TOO_SMALL == 8, "FERRIC_ERROR_BUFFER_TOO_SMALL must be 8");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_INVALID_ARGUMENT == 9, "FERRIC_ERROR_INVALID_ARGUMENT must be 9");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_SERIALIZATION_ERROR == 10,
                     "FERRIC_ERROR_SERIALIZATION_ERROR must be 10");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_PINNED_CLOSED == 11, "FERRIC_ERROR_PINNED_CLOSED must be 11");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_PINNED_CANCELED == 12, "FERRIC_ERROR_PINNED_CANCELED must be 12");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_PINNED_QUEUE_FULL == 13,
                     "FERRIC_ERROR_PINNED_QUEUE_FULL must be 13");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_PINNED_DISPATCH_FAILED == 14,
                     "FERRIC_ERROR_PINNED_DISPATCH_FAILED must be 14");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_PINNED_REENTRANT_CALL == 15,
                     "FERRIC_ERROR_PINNED_REENTRANT_CALL must be 15");
FERRIC_STATIC_ASSERT(FERRIC_ERROR_INTERNAL_ERROR == 99, "FERRIC_ERROR_INTERNAL_ERROR must be 99");

#if defined(FERRIC_SERDE)
/* FerricSerializationFormat: enum object width and stable numeric values. */
FERRIC_STATIC_ASSERT(sizeof(enum FerricSerializationFormat) == 4,
                     "enum FerricSerializationFormat must be 32 bits");
FERRIC_STATIC_ASSERT(FERRIC_SERIALIZATION_FORMAT_BINCODE == 0,
                     "FERRIC_SERIALIZATION_FORMAT_BINCODE must be 0");
FERRIC_STATIC_ASSERT(FERRIC_SERIALIZATION_FORMAT_JSON == 1,
                     "FERRIC_SERIALIZATION_FORMAT_JSON must be 1");
FERRIC_STATIC_ASSERT(FERRIC_SERIALIZATION_FORMAT_CBOR == 2,
                     "FERRIC_SERIALIZATION_FORMAT_CBOR must be 2");
FERRIC_STATIC_ASSERT(FERRIC_SERIALIZATION_FORMAT_MESSAGE_PACK == 3,
                     "FERRIC_SERIALIZATION_FORMAT_MESSAGE_PACK must be 3");
FERRIC_STATIC_ASSERT(FERRIC_SERIALIZATION_FORMAT_POSTCARD == 4,
                     "FERRIC_SERIALIZATION_FORMAT_POSTCARD must be 4");
#endif

"#;

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
        "ferric_engine_load_string(struct FerricEngine *engine, const char *source);",
        "ferric_engine_load_string(struct FerricEngine *engine, const char * FERRIC_NULL_TERMINATED source);",
    ),
    // ferric_engine_new_with_source: source is NUL-terminated.
    (
        "ferric_engine_new_with_source(const char *source);",
        "ferric_engine_new_with_source(const char * FERRIC_NULL_TERMINATED source);",
    ),
    // ferric_engine_new_with_source_config: source is NUL-terminated.
    // (source is the first param, on the same line as the function name)
    (
        "ferric_engine_new_with_source_config(const char *source,",
        "ferric_engine_new_with_source_config(const char * FERRIC_NULL_TERMINATED source,",
    ),
    // ferric_engine_assert_string: source is NUL-terminated.
    // (multi-line signature — source is on the continuation line)
    (
        "ferric_engine_assert_string(struct FerricEngine *engine,\n                                             const char *source,",
        "ferric_engine_assert_string(struct FerricEngine *engine,\n                                             const char * FERRIC_NULL_TERMINATED source,",
    ),
    // ferric_engine_get_output: channel is NUL-terminated.
    (
        "ferric_engine_get_output(const struct FerricEngine *engine, const char *channel);",
        "ferric_engine_get_output(const struct FerricEngine *engine, const char * FERRIC_NULL_TERMINATED channel);",
    ),
    // ferric_engine_clear_output: channel is NUL-terminated.
    (
        "ferric_engine_clear_output(struct FerricEngine *engine, const char *channel);",
        "ferric_engine_clear_output(struct FerricEngine *engine, const char * FERRIC_NULL_TERMINATED channel);",
    ),
    // ferric_engine_get_global: name is NUL-terminated.
    // (multi-line signature — name is on the continuation line)
    (
        "ferric_engine_get_global(const struct FerricEngine *engine,\n                                          const char *name,",
        "ferric_engine_get_global(const struct FerricEngine *engine,\n                                          const char * FERRIC_NULL_TERMINATED name,",
    ),
    // ferric_engine_push_input: line is NUL-terminated.
    (
        "ferric_engine_push_input(struct FerricEngine *engine, const char *line);",
        "ferric_engine_push_input(struct FerricEngine *engine, const char * FERRIC_NULL_TERMINATED line);",
    ),
    // ferric_engine_find_fact_ids: relation is NUL-terminated.
    // (multi-line signature — relation is on the continuation line)
    (
        "ferric_engine_find_fact_ids(const struct FerricEngine *engine,\n                                             const char *relation,",
        "ferric_engine_find_fact_ids(const struct FerricEngine *engine,\n                                             const char * FERRIC_NULL_TERMINATED relation,",
    ),
    // ferric_engine_assert_ordered: relation is NUL-terminated.
    // (multi-line signature — relation is on the continuation line)
    (
        "ferric_engine_assert_ordered(struct FerricEngine *engine,\n                                              const char *relation,",
        "ferric_engine_assert_ordered(struct FerricEngine *engine,\n                                              const char * FERRIC_NULL_TERMINATED relation,",
    ),
    // ferric_engine_template_slot_count: template_name is NUL-terminated.
    // (multi-line signature — template_name is on the continuation line)
    (
        "ferric_engine_template_slot_count(const struct FerricEngine *engine,\n                                                   const char *template_name,",
        "ferric_engine_template_slot_count(const struct FerricEngine *engine,\n                                                   const char * FERRIC_NULL_TERMINATED template_name,",
    ),
    // ferric_engine_template_slot_name: template_name is NUL-terminated.
    // (multi-line signature — template_name is on the continuation line)
    (
        "ferric_engine_template_slot_name(const struct FerricEngine *engine,\n                                                  const char *template_name,",
        "ferric_engine_template_slot_name(const struct FerricEngine *engine,\n                                                  const char * FERRIC_NULL_TERMINATED template_name,",
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
    // ferric_pinned_engine_last_error_copy: buf is a byte buffer of buf_len bytes.
    // (multi-line signature — pattern spans the line break)
    (
        "ferric_pinned_engine_last_error_copy(const struct FerricPinnedEngine *engine,\n                                                      char *buf,",
        "ferric_pinned_engine_last_error_copy(const struct FerricPinnedEngine *engine,\n                                                      char *buf FERRIC_SIZED_BY(buf_len),",
    ),
    // ferric_engine_action_diagnostic_copy: buf is a byte buffer of buf_len bytes.
    // (multi-line signature — pattern spans the line break)
    (
        "uintptr_t index,\n                                                      char *buf,",
        "uintptr_t index,\n                                                      char *buf FERRIC_SIZED_BY(buf_len),",
    ),
    // ferric_engine_assert_ordered: fields is an array of field_count FerricValues.
    // (multi-line signature — fields is on the continuation line)
    (
        "const struct FerricValue *fields,\n                                              uintptr_t field_count,",
        "const struct FerricValue *fields FERRIC_COUNTED_BY(field_count),\n                                              uintptr_t field_count,",
    ),
    // ferric_engine_get_fact_relation: buf is a byte buffer of buf_len bytes.
    (
        "uint64_t fact_id,\n                                                 char *buf,\n                                                 uintptr_t buf_len,\n                                                 uintptr_t *out_len);",
        "uint64_t fact_id,\n                                                 char *buf FERRIC_SIZED_BY(buf_len),\n                                                 uintptr_t buf_len,\n                                                 uintptr_t *out_len);",
    ),
    // ferric_engine_get_fact_template_name: buf is a byte buffer of buf_len bytes.
    (
        "uint64_t fact_id,\n                                                      char *buf,\n                                                      uintptr_t buf_len,\n                                                      uintptr_t *out_len);",
        "uint64_t fact_id,\n                                                      char *buf FERRIC_SIZED_BY(buf_len),\n                                                      uintptr_t buf_len,\n                                                      uintptr_t *out_len);",
    ),
    // ferric_engine_template_name: buf is a byte buffer of buf_len bytes.
    (
        "ferric_engine_template_name(const struct FerricEngine *engine,\n                                             uintptr_t index,\n                                             char *buf,",
        "ferric_engine_template_name(const struct FerricEngine *engine,\n                                             uintptr_t index,\n                                             char *buf FERRIC_SIZED_BY(buf_len),",
    ),
    // ferric_engine_template_slot_name: buf is a byte buffer of buf_len bytes.
    (
        "uintptr_t slot_index,\n                                                  char *buf,",
        "uintptr_t slot_index,\n                                                  char *buf FERRIC_SIZED_BY(buf_len),",
    ),
    // ferric_engine_rule_info: buf is a byte buffer of buf_len bytes.
    (
        "ferric_engine_rule_info(const struct FerricEngine *engine,\n                                         uintptr_t index,\n                                         char *buf,",
        "ferric_engine_rule_info(const struct FerricEngine *engine,\n                                         uintptr_t index,\n                                         char *buf FERRIC_SIZED_BY(buf_len),",
    ),
    // ferric_engine_current_module: buf is a byte buffer of buf_len bytes.
    (
        "ferric_engine_current_module(const struct FerricEngine *engine,\n                                              char *buf,",
        "ferric_engine_current_module(const struct FerricEngine *engine,\n                                              char *buf FERRIC_SIZED_BY(buf_len),",
    ),
    // ferric_engine_get_focus: buf is a byte buffer of buf_len bytes.
    (
        "ferric_engine_get_focus(const struct FerricEngine *engine,\n                                         char *buf,",
        "ferric_engine_get_focus(const struct FerricEngine *engine,\n                                         char *buf FERRIC_SIZED_BY(buf_len),",
    ),
    // ferric_engine_focus_stack_entry: buf is a byte buffer of buf_len bytes.
    (
        "uintptr_t index,\n                                                 char *buf,\n                                                 uintptr_t buf_len,\n                                                 uintptr_t *out_len);",
        "uintptr_t index,\n                                                 char *buf FERRIC_SIZED_BY(buf_len),\n                                                 uintptr_t buf_len,\n                                                 uintptr_t *out_len);",
    ),
    // ferric_engine_module_name: buf is a byte buffer of buf_len bytes.
    (
        "ferric_engine_module_name(const struct FerricEngine *engine,\n                                           uintptr_t index,\n                                           char *buf,",
        "ferric_engine_module_name(const struct FerricEngine *engine,\n                                           uintptr_t index,\n                                           char *buf FERRIC_SIZED_BY(buf_len),",
    ),
    // ferric_engine_fact_ids: out_ids is an array of max_ids u64s.
    (
        "ferric_engine_fact_ids(const struct FerricEngine *engine,\n                                        uint64_t *out_ids,",
        "ferric_engine_fact_ids(const struct FerricEngine *engine,\n                                        uint64_t *out_ids FERRIC_COUNTED_BY(max_ids),",
    ),
    // ferric_engine_find_fact_ids: out_ids is an array of max_ids u64s.
    (
        "const char * FERRIC_NULL_TERMINATED relation,\n                                             uint64_t *out_ids,",
        "const char * FERRIC_NULL_TERMINATED relation,\n                                             uint64_t *out_ids FERRIC_COUNTED_BY(max_ids),",
    ),
    // ferric_value_symbol: name is NUL-terminated.
    (
        "ferric_value_symbol(const char *name)",
        "ferric_value_symbol(const char * FERRIC_NULL_TERMINATED name)",
    ),
    // ferric_value_string: s is NUL-terminated.
    (
        "ferric_value_string(const char *s)",
        "ferric_value_string(const char * FERRIC_NULL_TERMINATED s)",
    ),
    // ferric_engine_assert_template: template_name is NUL-terminated.
    // (multi-line signature — template_name is on the continuation line)
    (
        "ferric_engine_assert_template(struct FerricEngine *engine,\n                                               const char *template_name,",
        "ferric_engine_assert_template(struct FerricEngine *engine,\n                                               const char * FERRIC_NULL_TERMINATED template_name,",
    ),
    // ferric_engine_assert_template: slot_names is an array of count pointers.
    (
        "const char *const *slot_names,\n                                               const struct FerricValue *slot_values,\n                                               uintptr_t count,",
        "const char *const *slot_names FERRIC_COUNTED_BY(count),\n                                               const struct FerricValue *slot_values FERRIC_COUNTED_BY(count),\n                                               uintptr_t count,",
    ),
    // ferric_engine_get_fact_slot_by_name: slot_name is NUL-terminated.
    // (multi-line signature — slot_name is on the continuation line)
    (
        "ferric_engine_get_fact_slot_by_name(const struct FerricEngine *engine,\n                                                     uint64_t fact_id,\n                                                     const char *slot_name,",
        "ferric_engine_get_fact_slot_by_name(const struct FerricEngine *engine,\n                                                     uint64_t fact_id,\n                                                     const char * FERRIC_NULL_TERMINATED slot_name,",
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

    // Append ABI static assertions immediately before the closing include
    // guard so all asserted types are already declared.
    let guard_close = "#endif  /* FERRIC_H */";
    let guard_pos = header
        .rfind(guard_close)
        .expect("Could not find closing include guard in generated header");
    header.insert_str(guard_pos, STATIC_ASSERTIONS);

    std::fs::write(format!("{crate_dir}/ferric.h"), header).expect("Failed to write ferric.h");
}
