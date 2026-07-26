/*
 * ferric.h - C API for the Ferric rules engine
 *
 * ============================================================
 * THREAD SAFETY
 * ============================================================
 *
 * Raw engine handles (FerricEngine*) are bound to the thread that
 * created them. Ordinary ferric_engine_* runtime accessors validate
 * thread affinity before accessing runtime state.
 *
 * - Creating thread: all operations succeed normally.
 * - Other threads: ordinary runtime operations return
 *   FERRIC_ERROR_THREAD_VIOLATION with a descriptive message in the
 *   global error channel.
 * - ferric_engine_last_error_copy() is synchronized and may run
 *   concurrently from any thread. Each call copies one coherent
 *   error snapshot.
 * - ferric_engine_last_error() may be called from any thread, but
 *   the returned borrowed pointer must not be used while another
 *   borrowed read or engine destruction may occur. Use the copy API
 *   when pointer-use windows could overlap.
 * - ferric_engine_free_unchecked() is a destruction-only escape
 *   hatch that deliberately skips affinity. Like all destruction,
 *   it must not overlap any access to that engine.
 * - Neither diagnostic reader may race with engine destruction.
 * - Same-engine runtime reentry from a host callback fails with
 *   FERRIC_ERROR_INTERNAL_ERROR. The last-error readers remain safe
 *   to call from such callbacks.
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
 * 2. Borrowed error pointers: ferric_last_error_global() remains
 *    valid until the next call that may modify that thread's global
 *    error channel. ferric_engine_last_error() remains valid until
 *    the next borrowed read on that engine or engine destruction;
 *    error writers and the copy API do not invalidate it. Do NOT
 *    free either pointer.
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
 */

#ifndef FERRIC_H
#define FERRIC_H

/* Warning: this file is autogenerated by cbindgen. Do not modify manually. */

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
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


// C-facing error codes returned by all fallible FFI entry points.
//
// Stable numeric values — new codes may be added but existing values
// must never change.
//
// Numeric ranges:
//
// - `0`           : success
// - `1..=10`      : raw engine + pre-pinned errors (stable)
// - `11..=19`     : pinned-execution errors (reserved range)
// - `20..=98`     : reserved for future growth
// - `99`          : internal/unexpected error
typedef enum FerricError {
    // Operation succeeded.
    FERRIC_ERROR_OK = 0,
    // A required pointer argument was null.
    FERRIC_ERROR_NULL_POINTER = 1,
    // Engine called from wrong thread.
    FERRIC_ERROR_THREAD_VIOLATION = 2,
    // Requested fact/item not found.
    FERRIC_ERROR_NOT_FOUND = 3,
    // Parse error in CLIPS source.
    FERRIC_ERROR_PARSE_ERROR = 4,
    // Compilation/validation error.
    FERRIC_ERROR_COMPILE_ERROR = 5,
    // Runtime evaluation error.
    FERRIC_ERROR_RUNTIME_ERROR = 6,
    // I/O error (file not found, etc).
    FERRIC_ERROR_IO_ERROR = 7,
    // Provided buffer too small for result.
    FERRIC_ERROR_BUFFER_TOO_SMALL = 8,
    // Invalid argument value.
    FERRIC_ERROR_INVALID_ARGUMENT = 9,
    // Serialization or deserialization error.
    FERRIC_ERROR_SERIALIZATION_ERROR = 10,
    // Pinned engine handle has been closed.
    FERRIC_ERROR_PINNED_CLOSED = 11,
    // Pinned engine request was canceled before completion.
    FERRIC_ERROR_PINNED_CANCELED = 12,
    // Pinned engine bounded queue rejected a new request (queue full).
    FERRIC_ERROR_PINNED_QUEUE_FULL = 13,
    // Pinned engine worker thread stopped unexpectedly (panicked or vanished).
    FERRIC_ERROR_PINNED_DISPATCH_FAILED = 14,
    // A synchronous pinned call was attempted from that engine's worker thread.
    FERRIC_ERROR_PINNED_REENTRANT_CALL = 15,
    // Internal/unexpected error.
    FERRIC_ERROR_INTERNAL_ERROR = 99
} FerricError;

// C-facing fact type discriminant.
typedef enum FerricFactType {
    FERRIC_FACT_TYPE_ORDERED = 0,
    FERRIC_FACT_TYPE_TEMPLATE = 1
} FerricFactType;

// C-facing halt reason returned by `ferric_engine_run_ex`.
typedef enum FerricHaltReason {
    FERRIC_HALT_REASON_AGENDA_EMPTY = 0,
    FERRIC_HALT_REASON_LIMIT_REACHED = 1,
    FERRIC_HALT_REASON_HALT_REQUESTED = 2
} FerricHaltReason;

// C-facing string-encoding configuration for `FerricConfig`.
typedef enum FerricStringEncoding {
    FERRIC_STRING_ENCODING_ASCII = 0,
    FERRIC_STRING_ENCODING_UTF8 = 1,
    FERRIC_STRING_ENCODING_ASCII_SYMBOLS_UTF8_STRINGS = 2
} FerricStringEncoding;

// C-facing conflict-resolution strategy for `FerricConfig`.
typedef enum FerricConflictStrategy {
    FERRIC_CONFLICT_STRATEGY_DEPTH = 0,
    FERRIC_CONFLICT_STRATEGY_BREADTH = 1,
    FERRIC_CONFLICT_STRATEGY_LEX = 2,
    FERRIC_CONFLICT_STRATEGY_MEA = 3
} FerricConflictStrategy;

// C-facing value type discriminant.
//
// Crosses the ABI as a raw `u32` (`FerricValue::value_type`), never as this
// Rust enum: caller-populated memory is validated with [`Self::from_raw`] /
// `TryFrom<u32>` before being interpreted, and unknown discriminants are
// rejected with `FERRIC_ERROR_INVALID_ARGUMENT`.
//
// Stable numeric values — new variants may be added but existing values
// must never change.
typedef enum FerricValueType {
    FERRIC_VALUE_TYPE_VOID = 0,
    FERRIC_VALUE_TYPE_INTEGER = 1,
    FERRIC_VALUE_TYPE_FLOAT = 2,
    FERRIC_VALUE_TYPE_SYMBOL = 3,
    FERRIC_VALUE_TYPE_STRING = 4,
    FERRIC_VALUE_TYPE_MULTIFIELD = 5,
    FERRIC_VALUE_TYPE_EXTERNAL_ADDRESS = 6
} FerricValueType;

#if defined(FERRIC_SERDE)
// Serialization format selector for `ferric_engine_serialize_as` and
// `ferric_engine_deserialize_as`.
typedef enum FerricSerializationFormat {
    // Compact binary (bincode). Fast and small.
    FERRIC_SERIALIZATION_FORMAT_BINCODE = 0,
    // JSON (human-readable, larger output).
    FERRIC_SERIALIZATION_FORMAT_JSON = 1,
    // CBOR (Concise Binary Object Representation).
    FERRIC_SERIALIZATION_FORMAT_CBOR = 2,
    // `MessagePack` (compact binary, JSON-like schema).
    FERRIC_SERIALIZATION_FORMAT_MESSAGE_PACK = 3,
    // Postcard (compact, `no_std`-friendly binary).
    FERRIC_SERIALIZATION_FORMAT_POSTCARD = 4
} FerricSerializationFormat;
#endif

// Autorelease-pool installation policy used by the pinned worker.
typedef enum FerricPinnedAutoreleasePolicy {
    // Never install an Apple autorelease pool.
    FERRIC_PINNED_AUTORELEASE_POLICY_NONE = 0,
    // Install one pool per drained request.
    FERRIC_PINNED_AUTORELEASE_POLICY_PER_ITEM = 1,
    // Install one pool per drained batch.
    FERRIC_PINNED_AUTORELEASE_POLICY_PER_BATCH = 2
} FerricPinnedAutoreleasePolicy;

// Opaque engine handle exposed to C.
//
// The runtime engine remains owner-thread-only. Per-engine error snapshots
// are stored separately so the two last-error accessors can safely read them
// from any thread.
//
// Production accessors never construct a Rust reference to this whole
// structure. They project references to individual fields from the raw
// handle, which permits an owner-thread `&mut Engine` to coexist with a
// foreign-thread reference to the disjoint diagnostic mutex.
//
// C code receives `*mut FerricEngine` as an opaque pointer.
typedef struct FerricEngine FerricEngine;

// Opaque handle to a Rust-owned pinned engine. Cloning is not supported;
// the FFI handle is the unique owner of its worker thread.
typedef struct FerricPinnedEngine FerricPinnedEngine;

// Opaque handle to an async-operation result. Caller must free with
// [`ferric_pinned_result_free`].
typedef struct FerricPinnedResult FerricPinnedResult;

// C-facing engine configuration used by `ferric_engine_new_with_config`.
typedef struct FerricConfig {
    // Raw `FerricStringEncoding` discriminant.
    uint32_t string_encoding;
    // Raw `FerricConflictStrategy` discriminant.
    uint32_t strategy;
    uintptr_t max_call_depth;
} FerricConfig;

// C-facing value representation.
//
// ## Ownership
//
// - `string_ptr`: when non-null, is a heap-allocated NUL-terminated string.
//   The caller must free it with `ferric_string_free` or `ferric_value_free`.
// - `multifield_ptr`: when non-null, is a heap-allocated array of `FerricValue`s.
//   The caller must free it with `ferric_value_free` (which recursively frees elements)
//   or `ferric_value_array_free`.
// - `external_pointer`: NOT owned by `FerricValue`. Lifetime is caller-managed.
//
// ## Active Fields by Type
//
// | `value_type` | Active fields |
// |---|---|
// | Void | (none) |
// | Integer | `integer` |
// | Float | `float` |
// | Symbol | `string_ptr` (owned) |
// | String | `string_ptr` (owned) |
// | Multifield | `multifield_ptr` (owned), `multifield_len` |
// | ExternalAddress | `external_type_id`, `external_pointer` |
typedef struct FerricValue {
    // Raw `FerricValueType` discriminant.
    //
    // Every API that reads a caller-populated `FerricValue` validates this
    // field before interpreting it; values outside the documented
    // `FerricValueType` range are rejected with
    // `FERRIC_ERROR_INVALID_ARGUMENT`.
    uint32_t value_type;
    int64_t integer;
    double float_;
    char * FERRIC_NULL_TERMINATED string_ptr;
    struct FerricValue *multifield_ptr FERRIC_COUNTED_BY(multifield_len);
    uintptr_t multifield_len;
    uint32_t external_type_id;
    void *external_pointer;
} FerricValue;

#if defined(FERRIC_SERDE)
// Callback type for caller-controlled memory allocation.
//
// When non-null, called by serialization functions with the exact byte
// count needed. The `context` parameter is passed through unchanged from
// the serialize call.
//
// Must return a pointer to at least `size` writable bytes, or null to
// signal allocation failure.
//
// The callback may query the same raw engine's last-error channel. Other
// same-engine runtime calls are rejected with `InternalError` while
// serialization is active, and the callback must not free the engine.
typedef uint8_t *(*FerricAllocFn)(uintptr_t size, void *context);
#endif

// C-facing options struct for [`ferric_pinned_engine_new`].
//
// Zero / NULL values are interpreted as "use default" (see the corresponding
// fields on [`ferric_pinned::PinnedEngineOptions`]). In particular, an
// all-zero [`FerricConfig`] selects [`ferric_runtime::EngineConfig::default`].
typedef struct FerricPinnedEngineOptions {
    // Inner engine configuration. All fields zero selects the default UTF-8,
    // Depth-strategy configuration with a call-depth limit of 64.
    struct FerricConfig engine;
    // Raw [`FerricPinnedAutoreleasePolicy`] discriminant.
    uint32_t autorelease_policy;
    // Maximum drain-batch size. `0` ⇒ drain everything available.
    uintptr_t max_batch_size;
    // Bounded request-queue capacity. `0` ⇒ default.
    uintptr_t queue_capacity;
    // Worker thread name (NUL-terminated). NULL ⇒ default.
    const char *thread_name;
} FerricPinnedEngineOptions;

// C completion-callback type. The result handle is non-NULL and owned by
// the caller; the caller can read its echoed request ID with
// [`ferric_pinned_result_request_id`] and must release it with
// [`ferric_pinned_result_free`].
//
// **Threading contract**: the callback runs on the Rust pinned worker
// thread. It must be transport-only — resume a continuation, signal an
// event, post to an actor — and must not call back into the same
// `FerricPinnedEngine` synchronously or perform long work.
// It must return normally and must not unwind across the FFI boundary.
typedef void (*FerricPinnedCompletionFn)(void *context,
                                         enum FerricError code,
                                         struct FerricPinnedResult *result);

// Create a new engine with default configuration.
//
// Returns a heap-allocated engine handle, or null on failure.
// The caller owns the returned handle and must free it with
// `ferric_engine_free`.
//
// # Safety
//
// The returned pointer must be freed with `ferric_engine_free`.
// The engine is bound to the creating thread.
struct FerricEngine *ferric_engine_new(void);

// Create a new engine with optional caller-provided configuration.
//
// If `config` is null, defaults are used.
//
// # Safety
//
// - `config` may be null.
// - Returned pointer must be freed with `ferric_engine_free`.
struct FerricEngine *ferric_engine_new_with_config(const struct FerricConfig *config);

// Free an engine handle.
//
// Null pointers are safely ignored. After this call, the pointer
// is invalid and must not be used.
//
// # Safety
//
// - `engine` must be a pointer returned by `ferric_engine_new` or null.
// - The engine must not be in use by another call when freed.
// - The engine must be freed from the same thread that created it.
enum FerricError ferric_engine_free(struct FerricEngine *engine);

// Load a CLIPS source string into the engine.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `source` must be a valid NUL-terminated UTF-8 string.
enum FerricError ferric_engine_load_string(struct FerricEngine *engine, const char * FERRIC_NULL_TERMINATED source);

// Retrieve the last per-engine error message.
//
// Returns a pointer to a NUL-terminated string, or null if no error is
// stored. This accessor may be called from any thread, including from a host
// callback.
//
// The pointer is borrowed from the engine and may be invalidated by the next
// call to `ferric_engine_last_error` on the same engine or by freeing the
// engine. Do not dereference or otherwise use the pointer while another
// borrowed read or engine destruction may occur. Use
// `ferric_engine_last_error_copy` when pointer-use windows could overlap.
//
// # Safety
//
// - `engine` must be a valid engine pointer or null.
const char * FERRIC_NULL_TERMINATED ferric_engine_last_error(const struct FerricEngine *engine);

// Copy the per-engine error message into a caller-provided buffer.
//
// Same contract as `ferric_last_error_global_copy` but reads from the
// per-engine error channel. This accessor may be called concurrently from
// any thread and from a host callback. Each invocation observes and copies
// one coherent error snapshot.
//
// A size query and a later copy are separate snapshots. If the error changes
// between those calls, the copy may return `BufferTooSmall` with the newer
// required size; callers should resize and retry.
//
// ## Contract
//
// | Condition | Return | `*out_len` |
// |-----------|--------|------------|
// | `engine` is null | `NullPointer` | 0 |
// | No error stored | `NotFound` | 0 |
// | `out_len` is null | `InvalidArgument` | (not written) |
// | `buf` is null AND `buf_len` is 0 (size query) | `Ok` | required size (incl. NUL) |
// | `buf` non-null, `buf_len` >= needed | `Ok` | bytes written (incl. NUL) |
// | `buf` non-null, `buf_len` < needed | `BufferTooSmall` | full needed size (incl. NUL) |
//
// # Safety
//
// - `engine` must be a valid engine pointer or null (null → `NullPointer`).
// - `buf` must point to `buf_len` writable bytes, or be null for size query.
// - `out_len` must be a valid pointer (non-null).
enum FerricError ferric_engine_last_error_copy(const struct FerricEngine *engine,
                                               char *buf FERRIC_SIZED_BY(buf_len),
                                               uintptr_t buf_len,
                                               uintptr_t *out_len);

// Clear the per-engine error state.
//
// # Safety
//
// - `engine` must be a valid engine pointer or null (null returns `NullPointer`).
enum FerricError ferric_engine_clear_error(struct FerricEngine *engine);

// Reset the engine to its initial state.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
enum FerricError ferric_engine_reset(struct FerricEngine *engine);

// Run the engine, executing rules until the agenda is empty, the limit is
// reached, or a halt action fires.
//
// - `limit`: Maximum rule firings. Pass `-1` for unlimited.
// - `out_fired`: If non-null, receives the number of rules fired.
//
// Returns `FerricError::Ok` on success.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_fired` may be null (output is simply not written).
enum FerricError ferric_engine_run(struct FerricEngine *engine, int64_t limit, uint64_t *out_fired);

// Execute a single rule firing step.
//
// - `out_status`: If non-null, receives: `1` = rule fired, `0` = agenda empty,
//   `-1` = halted.
//
// Returns `FerricError::Ok` on success.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_status` may be null.
enum FerricError ferric_engine_step(struct FerricEngine *engine, int32_t *out_status);

// Assert a fact from a CLIPS source string (e.g., `"(assert (color red))"`).
//
// The source is parsed as a top-level CLIPS form and evaluated. If
// `out_fact_id` is non-null and an assert occurred, it receives the first
// asserted fact's opaque ID. If no fact was asserted, `0` is written.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `source` must be a valid NUL-terminated UTF-8 string.
// - `out_fact_id` may be null.
enum FerricError ferric_engine_assert_string(struct FerricEngine *engine,
                                             const char * FERRIC_NULL_TERMINATED source,
                                             uint64_t *out_fact_id);

// Retract a fact by its opaque fact ID obtained from a previous assert.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `fact_id` must be a valid fact ID obtained from a previous assert.
enum FerricError ferric_engine_retract(struct FerricEngine *engine, uint64_t fact_id);

// Get the engine's captured output for a named channel (e.g., `"stdout"`).
//
// Returns a pointer to a NUL-terminated string, or null if the channel has
// no output, the engine pointer is null, or the channel pointer is null.
// The returned pointer is valid until the next call that writes to that channel.
//
// # Safety
//
// - `engine` must be a valid engine pointer or null.
// - `channel` must be a valid NUL-terminated UTF-8 string or null.
const char * FERRIC_NULL_TERMINATED ferric_engine_get_output(const struct FerricEngine *engine, const char * FERRIC_NULL_TERMINATED channel);

// Get the number of action diagnostics captured during recent execution.
//
// Diagnostics are collected by `run`/`step` when non-fatal action errors occur
// (for example module visibility failures surfaced as warnings).
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_count` must be a valid pointer.
enum FerricError ferric_engine_action_diagnostic_count(const struct FerricEngine *engine,
                                                       uintptr_t *out_count);

// Copy one action diagnostic message into a caller-provided buffer.
//
// Message selection is by zero-based index into the current action-diagnostic list.
// The copy contract matches `ferric_last_error_global_copy`.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `buf` must point to `buf_len` writable bytes, or be null for size query.
// - `out_len` must be a valid pointer (non-null).
enum FerricError ferric_engine_action_diagnostic_copy(const struct FerricEngine *engine,
                                                      uintptr_t index,
                                                      char *buf FERRIC_SIZED_BY(buf_len),
                                                      uintptr_t buf_len,
                                                      uintptr_t *out_len);

// Clear all stored action diagnostics.
//
// # Safety
//
// - `engine` must be a valid engine pointer or null (null returns `NullPointer`).
enum FerricError ferric_engine_clear_action_diagnostics(struct FerricEngine *engine);

// Get the count of user-visible facts in working memory.
//
// The synthetic `(initial-fact)` is excluded from the count.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_count` must be a valid pointer.
enum FerricError ferric_engine_fact_count(const struct FerricEngine *engine, uintptr_t *out_count);

// Get the number of fields in a fact.
//
// For ordered facts, returns the number of field values.
// For template facts, returns the number of slots.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_count` must be a valid pointer.
enum FerricError ferric_engine_get_fact_field_count(const struct FerricEngine *engine,
                                                    uint64_t fact_id,
                                                    uintptr_t *out_count);

// Get a single field from a fact as a `FerricValue`.
//
// For ordered facts, `index` is the field position (0-based).
// For template facts, `index` is the slot position (0-based).
//
// The returned `FerricValue` is written to `*out_value`. The caller owns
// any heap-allocated resources (`string_ptr`, `multifield_ptr`) and must free
// them with `ferric_value_free` or the type-specific free functions.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_value` must be a valid pointer to a `FerricValue`.
enum FerricError ferric_engine_get_fact_field(const struct FerricEngine *engine,
                                              uint64_t fact_id,
                                              uintptr_t index,
                                              struct FerricValue *out_value);

// Get a global variable's value.
//
// The name should NOT include the `?*` prefix/suffix — pass just the base name
// (e.g., `"x"` for `?*x*`).
//
// Module/global visibility resolution follows the runtime's standard rules.
// Ambiguity and not-found conditions produce runtime-authored diagnostics.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `name` must be a valid NUL-terminated UTF-8 string.
// - `out_value` must be a valid pointer to a `FerricValue`.
enum FerricError ferric_engine_get_global(const struct FerricEngine *engine,
                                          const char * FERRIC_NULL_TERMINATED name,
                                          struct FerricValue *out_value);

// Copy all user-visible fact IDs to a caller-provided array.
//
// - Size query: `out_ids == NULL && max_ids == 0` → `*out_count` receives total count.
// - Partial copy: copies up to `max_ids` IDs, `*out_count` always receives total count.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_count` must be a valid pointer.
// - If `out_ids` is non-null, it must point to space for at least `max_ids` `u64`s.
enum FerricError ferric_engine_fact_ids(const struct FerricEngine *engine,
                                        uint64_t *out_ids FERRIC_COUNTED_BY(max_ids),
                                        uintptr_t max_ids,
                                        uintptr_t *out_count);

// Find fact IDs by relation name.
//
// Same size-query pattern as `ferric_engine_fact_ids`.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `relation` must be a valid NUL-terminated string.
// - `out_count` must be a valid pointer.
// - If `out_ids` is non-null, it must point to space for at least `max_ids` `u64`s.
enum FerricError ferric_engine_find_fact_ids(const struct FerricEngine *engine,
                                             const char * FERRIC_NULL_TERMINATED relation,
                                             uint64_t *out_ids FERRIC_COUNTED_BY(max_ids),
                                             uintptr_t max_ids,
                                             uintptr_t *out_count);

// Discriminate ordered vs. template fact.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_type` must be a valid pointer.
enum FerricError ferric_engine_get_fact_type(const struct FerricEngine *engine,
                                             uint64_t fact_id,
                                             enum FerricFactType *out_type);

// Get the relation name for an ordered fact.
//
// Standard buffer copy pattern.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_len` must be a valid pointer.
// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
enum FerricError ferric_engine_get_fact_relation(const struct FerricEngine *engine,
                                                 uint64_t fact_id,
                                                 char *buf FERRIC_SIZED_BY(buf_len),
                                                 uintptr_t buf_len,
                                                 uintptr_t *out_len);

// Get the template name for a template fact.
//
// Standard buffer copy pattern.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_len` must be a valid pointer.
// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
enum FerricError ferric_engine_get_fact_template_name(const struct FerricEngine *engine,
                                                      uint64_t fact_id,
                                                      char *buf FERRIC_SIZED_BY(buf_len),
                                                      uintptr_t buf_len,
                                                      uintptr_t *out_len);

// Assert an ordered fact from structured values, bypassing CLIPS source parsing.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `relation` must be a valid NUL-terminated string.
// - If `fields` is non-null, it must point to `field_count` valid `FerricValue`s.
// - `out_fact_id` may be null.
enum FerricError ferric_engine_assert_ordered(struct FerricEngine *engine,
                                              const char * FERRIC_NULL_TERMINATED relation,
                                              const struct FerricValue *fields FERRIC_COUNTED_BY(field_count),
                                              uintptr_t field_count,
                                              uint64_t *out_fact_id);

// Get the number of registered templates.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_count` must be a valid pointer.
enum FerricError ferric_engine_template_count(const struct FerricEngine *engine,
                                              uintptr_t *out_count);

// Get the name of a template by zero-based index.
//
// Standard buffer copy pattern.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_len` must be a valid pointer.
// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
enum FerricError ferric_engine_template_name(const struct FerricEngine *engine,
                                             uintptr_t index,
                                             char *buf FERRIC_SIZED_BY(buf_len),
                                             uintptr_t buf_len,
                                             uintptr_t *out_len);

// Get the number of slots in a named template.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `template_name` must be a valid NUL-terminated string.
// - `out_count` must be a valid pointer.
enum FerricError ferric_engine_template_slot_count(const struct FerricEngine *engine,
                                                   const char * FERRIC_NULL_TERMINATED template_name,
                                                   uintptr_t *out_count);

// Get the name of a slot in a named template by zero-based index.
//
// Standard buffer copy pattern.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `template_name` must be a valid NUL-terminated string.
// - `out_len` must be a valid pointer.
// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
enum FerricError ferric_engine_template_slot_name(const struct FerricEngine *engine,
                                                  const char * FERRIC_NULL_TERMINATED template_name,
                                                  uintptr_t slot_index,
                                                  char *buf FERRIC_SIZED_BY(buf_len),
                                                  uintptr_t buf_len,
                                                  uintptr_t *out_len);

// Get the number of registered rules.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_count` must be a valid pointer.
enum FerricError ferric_engine_rule_count(const struct FerricEngine *engine, uintptr_t *out_count);

// Get the name and salience of a rule by zero-based index.
//
// The rule name is written to `buf` using the standard buffer copy pattern.
// Salience is written to `*out_salience` if non-null.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_len` must be a valid pointer.
// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
// - `out_salience` may be null.
enum FerricError ferric_engine_rule_info(const struct FerricEngine *engine,
                                         uintptr_t index,
                                         char *buf FERRIC_SIZED_BY(buf_len),
                                         uintptr_t buf_len,
                                         uintptr_t *out_len,
                                         int32_t *out_salience);

// Get the current module name.
//
// Standard buffer copy pattern.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_len` must be a valid pointer.
// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
enum FerricError ferric_engine_current_module(const struct FerricEngine *engine,
                                              char *buf FERRIC_SIZED_BY(buf_len),
                                              uintptr_t buf_len,
                                              uintptr_t *out_len);

// Get the name of the module at the top of the focus stack.
//
// Standard buffer copy pattern. Returns `NotFound` if the focus stack is empty.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_len` must be a valid pointer.
// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
enum FerricError ferric_engine_get_focus(const struct FerricEngine *engine,
                                         char *buf FERRIC_SIZED_BY(buf_len),
                                         uintptr_t buf_len,
                                         uintptr_t *out_len);

// Get the depth of the focus stack.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_depth` must be a valid pointer.
enum FerricError ferric_engine_focus_stack_depth(const struct FerricEngine *engine,
                                                 uintptr_t *out_depth);

// Get a focus stack entry by zero-based index.
//
// Index 0 = bottom of stack, last index = top (current focus).
// Standard buffer copy pattern.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_len` must be a valid pointer.
// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
enum FerricError ferric_engine_focus_stack_entry(const struct FerricEngine *engine,
                                                 uintptr_t index,
                                                 char *buf FERRIC_SIZED_BY(buf_len),
                                                 uintptr_t buf_len,
                                                 uintptr_t *out_len);

// Get the number of registered modules.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_count` must be a valid pointer.
enum FerricError ferric_engine_module_count(const struct FerricEngine *engine,
                                            uintptr_t *out_count);

// Get the name of a module by zero-based index.
//
// Standard buffer copy pattern.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_len` must be a valid pointer.
// - If `buf` is non-null, it must point to at least `buf_len` writable bytes.
enum FerricError ferric_engine_module_name(const struct FerricEngine *engine,
                                           uintptr_t index,
                                           char *buf FERRIC_SIZED_BY(buf_len),
                                           uintptr_t buf_len,
                                           uintptr_t *out_len);

// Get the number of activations on the agenda.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_count` must be a valid pointer.
enum FerricError ferric_engine_agenda_count(const struct FerricEngine *engine,
                                            uintptr_t *out_count);

// Check whether the engine is halted.
//
// Writes 1 to `*out_halted` if halted, 0 if not halted.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_halted` must be a valid pointer.
enum FerricError ferric_engine_is_halted(const struct FerricEngine *engine, int32_t *out_halted);

// Request the engine to halt.
//
// Always succeeds. Idempotent — halting an already-halted engine is a no-op.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
enum FerricError ferric_engine_halt(struct FerricEngine *engine);

// Push an input line for the engine's `read`/`readline` functions.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `line` must be a valid NUL-terminated string.
enum FerricError ferric_engine_push_input(struct FerricEngine *engine, const char * FERRIC_NULL_TERMINATED line);

// Reset the engine to a blank slate.
//
// Removes all facts, rules, templates, globals, functions, generics, and
// modules except MAIN. Always succeeds.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
enum FerricError ferric_engine_clear(struct FerricEngine *engine);

// Create an engine from CLIPS source with default configuration.
//
// Returns a heap-allocated engine handle, or null on parse/compile error
// (sets global error message). The engine has already been loaded and reset.
//
// # Safety
//
// - `source` must be a valid NUL-terminated UTF-8 string, or null.
// - Returned pointer must be freed with `ferric_engine_free`.
struct FerricEngine *ferric_engine_new_with_source(const char * FERRIC_NULL_TERMINATED source);

// Create an engine from CLIPS source with explicit configuration.
//
// If `config` is null, defaults are used.
// Returns null on parse/compile error (sets global error message).
//
// # Safety
//
// - `source` must be a valid NUL-terminated UTF-8 string, or null.
// - `config` may be null.
// - Returned pointer must be freed with `ferric_engine_free`.
struct FerricEngine *ferric_engine_new_with_source_config(const char * FERRIC_NULL_TERMINATED source,
                                                          const struct FerricConfig *config);

// Clear a specific output channel.
//
// Always succeeds — clearing a non-existent channel is a no-op.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `channel` must be a valid NUL-terminated string.
enum FerricError ferric_engine_clear_output(struct FerricEngine *engine, const char * FERRIC_NULL_TERMINATED channel);

// Extended run with halt reason output.
//
// Same limit semantics as `ferric_engine_run` (negative = unlimited).
// Additionally writes the halt reason to `*out_reason` if non-null.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_fired` may be null.
// - `out_reason` may be null.
enum FerricError ferric_engine_run_ex(struct FerricEngine *engine,
                                      int64_t limit,
                                      uint64_t *out_fired,
                                      enum FerricHaltReason *out_reason);

// Assert a template fact with named slots.
//
// Looks up the template by name, resolves slot names to positions,
// fills in defaults for unspecified slots, and asserts the fact.
//
// `slot_names` and `slot_values` must each point to `count` elements.
// Each `slot_names[i]` is a NUL-terminated C string naming a slot,
// and `slot_values[i]` is the corresponding value for that slot.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `template_name` must be a valid NUL-terminated string.
// - If `count > 0`, `slot_names` must point to `count` valid NUL-terminated string pointers.
// - If `count > 0`, `slot_values` must point to `count` valid `FerricValue`s.
// - `out_fact_id` may be null.
enum FerricError ferric_engine_assert_template(struct FerricEngine *engine,
                                               const char * FERRIC_NULL_TERMINATED template_name,
                                               const char *const *slot_names FERRIC_COUNTED_BY(count),
                                               const struct FerricValue *slot_values FERRIC_COUNTED_BY(count),
                                               uintptr_t count,
                                               uint64_t *out_fact_id);

// Get a template fact's slot value by name.
//
// The fact must be a template fact. For ordered facts, returns
// `FERRIC_ERROR_INVALID_ARGUMENT`. If the slot name is not found,
// returns `FERRIC_ERROR_NOT_FOUND`.
//
// The returned `FerricValue` is written to `*out_value`. The caller owns
// any heap-allocated resources and must free them with `ferric_value_free`.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `slot_name` must be a valid NUL-terminated string.
// - `out_value` must be a valid pointer to a `FerricValue`.
enum FerricError ferric_engine_get_fact_slot_by_name(const struct FerricEngine *engine,
                                                     uint64_t fact_id,
                                                     const char * FERRIC_NULL_TERMINATED slot_name,
                                                     struct FerricValue *out_value);

// Free an engine handle without checking thread affinity.
//
// This is intended for use by garbage-collected runtimes (Go, etc.) whose
// finalizers run on arbitrary threads. In normal usage, prefer
// `ferric_engine_free` which validates thread affinity.
//
// Null pointers are safely ignored.
//
// # Safety
//
// - `engine` must be a pointer returned by `ferric_engine_new` or null.
// - The engine must not be in use by another call when freed.
// - The caller must guarantee that no other thread is concurrently using this engine.
enum FerricError ferric_engine_free_unchecked(struct FerricEngine *engine);

#if defined(FERRIC_SERDE)
// Serialize engine state to bytes in the specified format.
//
// `format` is a `u32` corresponding to `FerricSerializationFormat` discriminants
// (0 = Bincode, 1 = JSON, 2 = CBOR, 3 = `MessagePack`, 4 = Postcard).
// Returns `FERRIC_ERROR_INVALID_ARGUMENT` for out-of-range values.
//
// See `ferric_engine_serialize_bincode` for memory allocation details.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_data` and `out_len` must be valid, non-null pointers.
// - If `alloc_fn` is non-null, it must return a valid pointer to `size` bytes
//   (or null to signal failure).
enum FerricError ferric_engine_serialize_as(const struct FerricEngine *engine,
                                            uint32_t format,
                                            FerricAllocFn alloc_fn,
                                            void *alloc_context,
                                            uint8_t **out_data,
                                            uintptr_t *out_len);
#endif

#if defined(FERRIC_SERDE)
// Deserialize an engine from bytes in the specified format.
//
// `format` is a `u32` corresponding to `FerricSerializationFormat` discriminants
// (0 = Bincode, 1 = JSON, 2 = CBOR, 3 = `MessagePack`, 4 = Postcard).
// Returns `FERRIC_ERROR_INVALID_ARGUMENT` for out-of-range values.
//
// See `ferric_engine_deserialize_bincode` for details.
//
// # Safety
//
// - `data` must point to `len` valid, readable bytes.
// - `out_engine` must be a valid, non-null pointer.
// - The returned engine must be freed with `ferric_engine_free`.
enum FerricError ferric_engine_deserialize_as(const uint8_t *data,
                                              uintptr_t len,
                                              uint32_t format,
                                              struct FerricEngine **out_engine);
#endif

#if defined(FERRIC_SERDE)
// Serialize engine state to bincode.
//
// ## Memory allocation
//
// - If `alloc_fn` is **non-null**: the callback is called once with the exact
//   byte count needed. The serialized data is written into the returned
//   buffer. The caller owns this memory and is responsible for freeing it
//   (via their own allocator). `alloc_context` is passed through unchanged.
//
// - If `alloc_fn` is **null**: Rust allocates the output buffer internally.
//   The caller must free it with `ferric_bytes_free(out_data, out_len)`.
//
// In both cases, `*out_data` and `*out_len` are set on success.
//
// # Safety
//
// - `engine` must be a valid engine pointer.
// - `out_data` and `out_len` must be valid, non-null pointers.
// - If `alloc_fn` is non-null, it must return a valid pointer to `size` bytes
//   (or null to signal failure).
enum FerricError ferric_engine_serialize_bincode(const struct FerricEngine *engine,
                                                 FerricAllocFn alloc_fn,
                                                 void *alloc_context,
                                                 uint8_t **out_data,
                                                 uintptr_t *out_len);
#endif

#if defined(FERRIC_SERDE)
// Deserialize an engine from bincode bytes.
//
// The returned engine handle is ready for use (e.g. `ferric_engine_run`).
// Its thread affinity is set to the calling thread.
//
// # Safety
//
// - `data` must point to `len` valid, readable bytes.
// - `out_engine` must be a valid, non-null pointer.
// - The returned engine must be freed with `ferric_engine_free`.
enum FerricError ferric_engine_deserialize_bincode(const uint8_t *data,
                                                   uintptr_t len,
                                                   struct FerricEngine **out_engine);
#endif

#if defined(FERRIC_SERDE)
// Serialize engine state to JSON.
//
// See `ferric_engine_serialize_bincode` for memory allocation details.
//
// # Safety
//
// Same safety requirements as `ferric_engine_serialize_bincode`.
enum FerricError ferric_engine_serialize_json(const struct FerricEngine *engine,
                                              FerricAllocFn alloc_fn,
                                              void *alloc_context,
                                              uint8_t **out_data,
                                              uintptr_t *out_len);
#endif

#if defined(FERRIC_SERDE)
// Deserialize an engine from JSON bytes.
//
// # Safety
//
// Same safety requirements as `ferric_engine_deserialize_bincode`.
enum FerricError ferric_engine_deserialize_json(const uint8_t *data,
                                                uintptr_t len,
                                                struct FerricEngine **out_engine);
#endif

#if defined(FERRIC_SERDE)
// Serialize engine state to CBOR.
//
// See `ferric_engine_serialize_bincode` for memory allocation details.
//
// # Safety
//
// Same safety requirements as `ferric_engine_serialize_bincode`.
enum FerricError ferric_engine_serialize_cbor(const struct FerricEngine *engine,
                                              FerricAllocFn alloc_fn,
                                              void *alloc_context,
                                              uint8_t **out_data,
                                              uintptr_t *out_len);
#endif

#if defined(FERRIC_SERDE)
// Deserialize an engine from CBOR bytes.
//
// # Safety
//
// Same safety requirements as `ferric_engine_deserialize_bincode`.
enum FerricError ferric_engine_deserialize_cbor(const uint8_t *data,
                                                uintptr_t len,
                                                struct FerricEngine **out_engine);
#endif

#if defined(FERRIC_SERDE)
// Serialize engine state to `MessagePack`.
//
// See `ferric_engine_serialize_bincode` for memory allocation details.
//
// # Safety
//
// Same safety requirements as `ferric_engine_serialize_bincode`.
enum FerricError ferric_engine_serialize_msgpack(const struct FerricEngine *engine,
                                                 FerricAllocFn alloc_fn,
                                                 void *alloc_context,
                                                 uint8_t **out_data,
                                                 uintptr_t *out_len);
#endif

#if defined(FERRIC_SERDE)
// Deserialize an engine from `MessagePack` bytes.
//
// # Safety
//
// Same safety requirements as `ferric_engine_deserialize_bincode`.
enum FerricError ferric_engine_deserialize_msgpack(const uint8_t *data,
                                                   uintptr_t len,
                                                   struct FerricEngine **out_engine);
#endif

#if defined(FERRIC_SERDE)
// Serialize engine state to Postcard.
//
// See `ferric_engine_serialize_bincode` for memory allocation details.
//
// # Safety
//
// Same safety requirements as `ferric_engine_serialize_bincode`.
enum FerricError ferric_engine_serialize_postcard(const struct FerricEngine *engine,
                                                  FerricAllocFn alloc_fn,
                                                  void *alloc_context,
                                                  uint8_t **out_data,
                                                  uintptr_t *out_len);
#endif

#if defined(FERRIC_SERDE)
// Deserialize an engine from Postcard bytes.
//
// # Safety
//
// Same safety requirements as `ferric_engine_deserialize_bincode`.
enum FerricError ferric_engine_deserialize_postcard(const uint8_t *data,
                                                    uintptr_t len,
                                                    struct FerricEngine **out_engine);
#endif

#if defined(FERRIC_SERDE)
// Free a byte buffer that was allocated by a serialize function when
// `alloc_fn` was null.
//
// Null pointers and zero lengths are safely ignored.
//
// # Safety
//
// - `data` must be a pointer returned by a serialize function (with
//   null `alloc_fn`), or null.
// - `len` must be the length reported by the corresponding serialize call.
// - The buffer must not have been previously freed.
void ferric_bytes_free(uint8_t *data, uintptr_t len);
#endif

// Retrieve the last global error message as a C string pointer.
//
// Returns a pointer to a NUL-terminated UTF-8 string, or null if no error
// is stored. The returned pointer is valid only until the next FFI call
// that may modify the global error channel.
//
// # Safety
//
// The returned pointer must not be freed by the caller and must not be
// used after any subsequent FFI call that may modify the error channel.
const char * FERRIC_NULL_TERMINATED ferric_last_error_global(void);

// Clear the global error channel.
void ferric_clear_error_global(void);

// Copy the last global error message into a caller-provided buffer.
//
// ## Contract
//
// | Condition | Return | `*out_len` |
// |-----------|--------|------------|
// | No error stored | `NotFound` | 0 |
// | `buf` is null AND `buf_len` is 0 (size query) | `Ok` | required size (incl. NUL) |
// | `buf` non-null, `buf_len` >= needed | `Ok` | bytes written (incl. NUL) |
// | `buf` non-null, `buf_len` < needed | `BufferTooSmall` | full needed size (incl. NUL) |
// | `buf_len` is 0 with non-null `buf` | `BufferTooSmall` | full needed size (incl. NUL) |
// | `out_len` is null | `InvalidArgument` | (not written) |
//
// When truncation occurs (`BufferTooSmall`), the buffer receives `buf_len - 1`
// bytes of the message followed by a NUL terminator. If `buf_len` is 0,
// nothing is written.
//
// # Safety
//
// - `buf` must point to `buf_len` writable bytes, or be null for size query.
// - `out_len` must be a valid pointer (non-null).
enum FerricError ferric_last_error_global_copy(char *buf FERRIC_SIZED_BY(buf_len), uintptr_t buf_len, uintptr_t *out_len);

// Construct a new pinned engine.
//
// Returns a heap-allocated handle on success, or NULL on failure (with the
// error message in the global error channel).
//
// # Safety
//
// - `options` must point to a valid [`FerricPinnedEngineOptions`] or be NULL.
// - The returned handle must be freed with [`ferric_pinned_engine_free`].
struct FerricPinnedEngine *ferric_pinned_engine_new(const struct FerricPinnedEngineOptions *options);

// Stop accepting requests, interrupt active and queued runs, drain any other
// already-queued requests, and join the worker. Interrupted runs complete
// with [`FerricHaltReason::HaltRequested`]. Idempotent.
//
// # Safety
//
// - `engine` must be a valid handle or NULL.
enum FerricError ferric_pinned_engine_close(struct FerricPinnedEngine *engine);

// Free a pinned engine handle. Closes it first if needed.
//
// # Safety
//
// - `engine` must be a pointer returned by [`ferric_pinned_engine_new`], or NULL.
// - The pointer must not be used after this call.
enum FerricError ferric_pinned_engine_free(struct FerricPinnedEngine *engine);

// Returns `true` once close has begun.
//
// # Safety
//
// - `engine` must be a valid handle (NULL ⇒ `false`).
bool ferric_pinned_engine_is_closed(const struct FerricPinnedEngine *engine);

// Request that the active run exit with `HaltRequested` at the next
// cancel-chunk boundary. Has no effect when no run is active and does not
// latch onto queued or future runs.
//
// # Safety
//
// - `engine` must be a valid handle.
enum FerricError ferric_pinned_engine_halt(struct FerricPinnedEngine *engine);

// Cancel a registered async request by ID.
//
// Ordinarily, a request waiting for queue capacity makes its submission call
// return [`FerricError::PinnedCanceled`] without firing its completion. An
// admitted pending request completes with [`FerricError::PinnedCanceled`].
// An in-flight run completes normally with
// [`FerricHaltReason::HaltRequested`].
//
// [`FerricError::Ok`] confirms only that cancellation was recorded.
// Cancellation does not guarantee that a concurrent submission succeeds.
// Queue timeout, close, or dispatch failure may win the race.
// A failed submission fires no completion.
//
// Returns [`FerricError::NotFound`] if the request is unknown, finished, or
// is a non-run operation that has already started.
//
// # Safety
//
// - `engine` must be a valid handle.
enum FerricError ferric_pinned_engine_cancel_request(struct FerricPinnedEngine *engine,
                                                     uint64_t request_id);

// Retrieve the last per-engine error message as a borrowed C string.
//
// Another call to this function may invalidate the returned pointer. Callers
// that can race with other threads should use
// [`ferric_pinned_engine_last_error_copy`] instead.
//
// # Safety
//
// - `engine` must be a valid handle (NULL ⇒ NULL return).
const char *ferric_pinned_engine_last_error(const struct FerricPinnedEngine *engine);

// Copy the last per-engine error message into a caller-provided buffer.
//
// Unlike [`ferric_pinned_engine_last_error`], the copied bytes are owned by
// the caller and cannot be invalidated by another thread reading the same
// pinned handle.
//
// ## Contract
//
// | Condition | Return | `*out_len` |
// |-----------|--------|------------|
// | `engine` is null | `NullPointer` | 0 |
// | No error stored | `NotFound` | 0 |
// | `out_len` is null | `InvalidArgument` | (not written) |
// | `buf` is null AND `buf_len` is 0 (size query) | `Ok` | required size (incl. NUL) |
// | `buf` non-null, `buf_len` >= needed | `Ok` | bytes written (incl. NUL) |
// | `buf` non-null, `buf_len` < needed | `BufferTooSmall` | full needed size (incl. NUL) |
//
// # Safety
//
// - `engine` must be a valid pinned engine pointer or null.
// - `buf` must point to `buf_len` writable bytes, or be null for a size query.
// - `out_len` must be a valid, non-null pointer.
enum FerricError ferric_pinned_engine_last_error_copy(const struct FerricPinnedEngine *engine,
                                                      char *buf FERRIC_SIZED_BY(buf_len),
                                                      uintptr_t buf_len,
                                                      uintptr_t *out_len);

// Load a CLIPS source string (synchronous).
//
// # Safety
//
// - `engine` must be a valid handle.
// - `source` must be a valid NUL-terminated UTF-8 string.
enum FerricError ferric_pinned_engine_load_string(struct FerricPinnedEngine *engine,
                                                  const char *source);

// Reset the engine state (synchronous).
//
// # Safety
//
// - `engine` must be a valid handle.
enum FerricError ferric_pinned_engine_reset(struct FerricPinnedEngine *engine);

// Clear the engine state (synchronous).
//
// # Safety
//
// - `engine` must be a valid handle.
enum FerricError ferric_pinned_engine_clear(struct FerricPinnedEngine *engine);

// Run the engine until the agenda is empty, the limit is reached, or halt is
// requested. Synchronous: blocks the caller until the worker completes.
//
// - `limit`: `-1` ⇒ unlimited; ≥ 0 ⇒ count limit.
// - `out_fired`: optional pointer to receive rules-fired count.
// - `out_reason`: optional pointer to receive halt reason.
//
// # Safety
//
// - `engine` must be a valid handle.
// - `out_fired` and `out_reason` may be NULL.
enum FerricError ferric_pinned_engine_run(struct FerricPinnedEngine *engine,
                                          int64_t limit,
                                          uint64_t *out_fired,
                                          enum FerricHaltReason *out_reason);

#if defined(FERRIC_SERDE)
// Serialize the engine state to the specified format (synchronous).
// Mirrors the allocator-callback contract of `ferric_engine_serialize_as`.
//
// # Safety
//
// - `engine` must be a valid handle.
// - `out_data` and `out_len` must be valid, non-null pointers.
// - If `alloc_fn` is non-null, see [`crate::engine::FerricAllocFn`].
enum FerricError ferric_pinned_engine_serialize_as(struct FerricPinnedEngine *engine,
                                                   uint32_t format,
                                                   FerricAllocFn alloc_fn,
                                                   void *alloc_context,
                                                   uint8_t **out_data,
                                                   uintptr_t *out_len);
#endif

// Submit a `run` asynchronously. Returns immediately on successful
// submission. `completion` fires on the worker thread when the operation
// completes (or fails).
//
// `request_id` identifies the pending request for
// [`ferric_pinned_engine_cancel_request`]. It must be unique among currently
// pending async requests for this engine, and is echoed on the completion
// result handle. Cancellation by ID remains available after the run starts.
//
// # Safety
//
// - `engine` must be a valid handle.
// - `completion` must be a callable function pointer.
// - `context` may be any pointer; the caller is responsible for ensuring it
//   is safe to access from the worker thread.
enum FerricError ferric_pinned_engine_run_async(struct FerricPinnedEngine *engine,
                                                int64_t limit,
                                                uint64_t request_id,
                                                void *context,
                                                FerricPinnedCompletionFn completion);

// Submit a `run` asynchronously, waiting only for bounded-queue capacity.
//
// `queue_wait_ms` controls admission: `-1` waits indefinitely, `0` retains
// fail-fast behavior, and a positive value waits up to that many
// milliseconds. The wait ends early if this request is canceled or the
// engine closes. Timeout expiry returns [`FerricError::PinnedQueueFull`]. Any
// synchronous error means the request was not admitted and `completion` will
// not fire. [`FerricError::Ok`] means the callback fires exactly once.
//
// # Safety
//
// The safety requirements are the same as
// [`ferric_pinned_engine_run_async`].
enum FerricError ferric_pinned_engine_run_async_wait_for_capacity(struct FerricPinnedEngine *engine,
                                                                  int64_t limit,
                                                                  uint64_t request_id,
                                                                  int64_t queue_wait_ms,
                                                                  void *context,
                                                                  FerricPinnedCompletionFn completion);

// Submit `load_str` asynchronously.
//
// `request_id` identifies the pending request for
// [`ferric_pinned_engine_cancel_request`]. It must be unique among currently
// pending async requests for this engine, and is echoed on the completion
// result handle.
//
// # Safety
//
// - `engine` must be a valid handle.
// - `source` must be a valid NUL-terminated UTF-8 string. The string is
//   copied; the caller may free it immediately after this call returns.
// - `completion` must be a callable function pointer.
enum FerricError ferric_pinned_engine_load_string_async(struct FerricPinnedEngine *engine,
                                                        const char *source,
                                                        uint64_t request_id,
                                                        void *context,
                                                        FerricPinnedCompletionFn completion);

// Submit `load_str` asynchronously, waiting only for bounded-queue capacity.
//
// `queue_wait_ms` controls admission: `-1` waits indefinitely, `0` retains
// fail-fast behavior, and a positive value waits up to that many
// milliseconds. The wait ends early if this request is canceled or the
// engine closes. Timeout expiry returns [`FerricError::PinnedQueueFull`]. Any
// synchronous error means the request was not admitted and `completion` will
// not fire. [`FerricError::Ok`] means the callback fires exactly once.
//
// # Safety
//
// The safety requirements are the same as
// [`ferric_pinned_engine_load_string_async`].
enum FerricError ferric_pinned_engine_load_string_async_wait_for_capacity(struct FerricPinnedEngine *engine,
                                                                          const char *source,
                                                                          uint64_t request_id,
                                                                          int64_t queue_wait_ms,
                                                                          void *context,
                                                                          FerricPinnedCompletionFn completion);

// Read the result's `FerricError` code.
//
// # Safety
//
// - `result` must be a valid handle returned via a completion callback.
enum FerricError ferric_pinned_result_code(const struct FerricPinnedResult *result);

// Read the result's echoed async request ID. Returns `0` for NULL.
//
// # Safety
//
// - `result` must be a valid handle returned via a completion callback.
uint64_t ferric_pinned_result_request_id(const struct FerricPinnedResult *result);

// Read a Run-typed result (`rules_fired` and halt reason).
//
// Returns [`FerricError::InvalidArgument`] if the result does not carry a Run payload.
//
// # Safety
//
// - `result` must be a valid handle.
// - `out_fired` and `out_reason` may be NULL.
enum FerricError ferric_pinned_result_get_run(const struct FerricPinnedResult *result,
                                              uint64_t *out_fired,
                                              enum FerricHaltReason *out_reason);

// Read the result's error message as a borrowed C string. Valid until
// [`ferric_pinned_result_free`] is called on the handle. Returns NULL
// if the result has no message.
//
// # Safety
//
// - `result` must be a valid handle.
const char *ferric_pinned_result_error_message(const struct FerricPinnedResult *result);

// Free a result handle. Idempotent for NULL.
//
// # Safety
//
// - `result` must be a handle obtained from a completion callback, or NULL.
// - The handle must not be used after this call.
void ferric_pinned_result_free(struct FerricPinnedResult *result);

// Create an integer `FerricValue`.
struct FerricValue ferric_value_integer(int64_t value);

// Create a float `FerricValue`.
struct FerricValue ferric_value_float(double value);

// Create a symbol `FerricValue` with a heap-copied string.
//
// Returns a void value if `name` is null. The caller owns the
// `string_ptr` and must free it with `ferric_value_free`.
//
// # Safety
//
// - `name` must be a valid NUL-terminated string, or null.
struct FerricValue ferric_value_symbol(const char * FERRIC_NULL_TERMINATED name);

// Create a string `FerricValue` with a heap-copied string.
//
// Returns a void value if `s` is null. The caller owns the
// `string_ptr` and must free it with `ferric_value_free`.
//
// # Safety
//
// - `s` must be a valid NUL-terminated string, or null.
struct FerricValue ferric_value_string(const char * FERRIC_NULL_TERMINATED s);

// Create a void `FerricValue` with all fields zeroed/null.
struct FerricValue ferric_value_void(void);

// Free a heap-allocated C string returned by the FFI.
//
// Null pointers are safely ignored.
//
// # Safety
//
// - `ptr` must be a pointer returned by an FFI function or null.
// - The pointer must not have been freed already.
void ferric_string_free(char * FERRIC_NULL_TERMINATED ptr);

// Free a `FerricValue` and its owned resources.
//
// Recursively frees owned strings and multifield arrays. Null pointers are
// safely ignored and return `FERRIC_ERROR_OK`.
//
// If the value (or any nested multifield element) carries an unknown
// `value_type` discriminant, returns `FERRIC_ERROR_INVALID_ARGUMENT` and
// records a diagnostic in the global error channel. The unknown-tagged
// value's payload fields are never interpreted (nothing of it is freed),
// but all sibling values with known discriminants and every owned
// containing multifield array are still released.
//
// # Safety
//
// - `value` must point to a valid `FerricValue` or be null.
// - Any owned resources (`string_ptr`, `multifield_ptr`) must not have been freed already.
enum FerricError ferric_value_free(struct FerricValue *value);

// Free an array of `FerricValue`s and all their owned resources.
//
// Frees each element's owned resources, then frees the array allocation.
// Null pointers are safely ignored and return `FERRIC_ERROR_OK`.
//
// If any element (or any nested multifield element) carries an unknown
// `value_type` discriminant, returns `FERRIC_ERROR_INVALID_ARGUMENT` and
// records a diagnostic in the global error channel. Unknown-tagged
// elements' payload fields are never interpreted (nothing of them is
// freed), but all elements with known discriminants and the array
// allocation itself are still released.
//
// # Safety
//
// - `arr` must point to a contiguous array of `len` `FerricValue`s, or be null.
// - The array must have been allocated by the FFI.
enum FerricError ferric_value_array_free(struct FerricValue *arr FERRIC_COUNTED_BY(len), uintptr_t len);


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

#endif  /* FERRIC_H */
