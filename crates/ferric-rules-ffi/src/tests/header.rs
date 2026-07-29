//! Header drift detection and smoke tests (Pass 008).
//!
//! These tests verify that the committed `ferric.h` exists and contains all
//! expected symbols, banners, and include guards.
//!
//! For full drift detection, run `cargo build -p ferric-rules-ffi` and then check
//! `git diff --exit-code crates/ferric-rules-ffi/ferric.h` in CI.

/// Read the committed `ferric.h` and return its contents.
///
/// Panics with a helpful message if the file is not found, guiding the
/// developer to run the build first.
fn read_committed_header() -> String {
    let crate_dir = env!("CARGO_MANIFEST_DIR");
    let header_path = format!("{crate_dir}/ferric.h");
    std::fs::read_to_string(&header_path).unwrap_or_else(|_| {
        panic!(
            "ferric.h not found at {header_path}. \
             Run `cargo build -p ferric-rules-ffi` to generate it."
        )
    })
}

fn read_committed_go_header() -> String {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let header_path = crate_dir.join("../../bindings/go/internal/ffi/lib/ferric.h");
    std::fs::read_to_string(&header_path)
        .unwrap_or_else(|_| panic!("Go FFI header not found at {}", header_path.display()))
}

fn read_ci_workflow() -> String {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow_path = crate_dir.join("../../.github/workflows/ci.yml");
    std::fs::read_to_string(&workflow_path)
        .unwrap_or_else(|_| panic!("CI workflow not found at {}", workflow_path.display()))
}

fn read_tsan_harness() -> String {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let script_path = crate_dir.join("../../scripts/ffi-tsan-harness.sh");
    std::fs::read_to_string(&script_path)
        .unwrap_or_else(|_| panic!("TSan harness not found at {}", script_path.display()))
}

fn read_pinned_async_tsan_harness() -> String {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let script_path = crate_dir.join("../../scripts/pinned-async-tsan.sh");
    std::fs::read_to_string(&script_path).unwrap_or_else(|_| {
        panic!(
            "pinned async TSan harness not found at {}",
            script_path.display()
        )
    })
}

fn read_panic_harness() -> String {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let script_path = crate_dir.join("../../scripts/ffi-panic-harness.sh");
    std::fs::read_to_string(&script_path)
        .unwrap_or_else(|_| panic!("panic harness not found at {}", script_path.display()))
}

#[test]
fn header_has_include_guard() {
    let header = read_committed_header();
    assert!(
        header.contains("FERRIC_H"),
        "ferric.h is missing the FERRIC_H include guard"
    );
}

#[test]
fn header_has_thread_safety_banner() {
    let header = read_committed_header();
    assert!(
        header.contains("THREAD SAFETY"),
        "ferric.h is missing the THREAD SAFETY section"
    );
    assert!(
        header.contains("FERRIC_ERROR_THREAD_VIOLATION"),
        "Thread-safety section must mention FERRIC_ERROR_THREAD_VIOLATION"
    );
    assert!(
        header.contains("ferric_engine_last_error_copy() is synchronized"),
        "Thread-safety section must promise synchronized owned snapshots"
    );
    assert!(
        header.contains("returned borrowed pointer must not be used while another"),
        "Thread-safety section must constrain borrowed-pointer use"
    );
    assert!(
        header.contains("ferric_engine_free_unchecked() is a destruction-only escape"),
        "Thread-safety section must document unchecked destruction"
    );
    assert!(
        header.contains("Neither diagnostic reader may race with engine destruction"),
        "Thread-safety section must forbid concurrent engine destruction"
    );
    assert!(
        header.contains("Same-engine runtime reentry from a host callback fails"),
        "Thread-safety section must document deterministic reentry rejection"
    );
}

#[test]
fn header_has_ownership_docs() {
    let header = read_committed_header();
    assert!(
        header.contains("OWNERSHIP AND LIFETIME"),
        "ferric.h is missing the OWNERSHIP AND LIFETIME section"
    );
    assert!(
        header.contains("ferric_engine_free"),
        "Ownership docs must mention ferric_engine_free"
    );
    assert!(
        header.contains("ferric_string_free"),
        "Ownership docs must mention ferric_string_free"
    );
    assert!(
        header.contains("ferric_value_free"),
        "Ownership docs must mention ferric_value_free"
    );
    assert!(
        header.contains("Borrowed value inputs: Value trees passed to structured"),
        "Ownership docs must identify structured value inputs as borrowed"
    );
    assert!(
        header.contains("Never pass stack or\n *    foreign-allocated value trees"),
        "Ownership docs must forbid freeing foreign value trees"
    );
}

#[test]
fn committed_ffi_headers_are_identical() {
    assert_eq!(
        read_committed_header(),
        read_committed_go_header(),
        "crate and Go binding copies of ferric.h must remain identical"
    );
}

#[test]
fn header_documents_active_only_pinned_halt() {
    let header = read_committed_header();
    assert!(
        header.contains("does not latch onto\n * queued or future runs"),
        "Pinned halt docs must state that idle calls do not latch"
    );
}

#[test]
fn header_contains_capacity_wait_async_entry_points() {
    let header = read_committed_header();
    assert!(
        header.contains("ferric_pinned_engine_run_async_wait_for_capacity"),
        "Missing capacity-waiting pinned run entry point"
    );
    assert!(
        header.contains("ferric_pinned_engine_load_string_async_wait_for_capacity"),
        "Missing capacity-waiting pinned load entry point"
    );
    assert!(
        header.contains("queue_wait_ms"),
        "Capacity-wait entry points must expose queue_wait_ms"
    );
    assert!(
        header.contains("request was not admitted") && header.contains("not fire."),
        "Capacity-wait docs must define the no-completion-on-rejection contract"
    );
}

#[test]
fn header_documents_cancel_and_submission_race() {
    let header = read_committed_header();
    assert!(
        header.contains("does not guarantee that a concurrent submission succeeds"),
        "Cancellation docs must weaken the concurrent-submission promise"
    );
    assert!(
        header.contains("failed submission fires no completion"),
        "Cancellation docs must retain the no-completion-on-rejection contract"
    );
}

#[test]
fn header_documents_exactly_once_async_terminal_contract() {
    let header = read_committed_header();
    for required in [
        "Every accepted operation removes its registry entry",
        "invokes completion exactly once",
        "contained operation panic reports",
        "FERRIC_ERROR_INTERNAL_ERROR",
        "request_id is reusable from the callback",
    ] {
        assert!(
            header.contains(required),
            "pinned async terminal contract is missing from ferric.h: {required}"
        );
    }
}

#[test]
fn header_documents_completion_callback_unwind_contract() {
    let header = read_committed_header();
    assert!(
        header.contains("must not unwind across the FFI boundary"),
        "Completion callback docs must forbid unwinding into Rust"
    );
}

#[test]
fn ci_checks_both_committed_headers_after_generation() {
    let workflow = read_ci_workflow();
    let build_position = workflow
        .find("- run: just build-go-ffi")
        .expect("CI must build the Go FFI before checking headers");
    let check_position = workflow
        .find(
            "git diff --exit-code -- crates/ferric-rules-ffi/ferric.h \
             bindings/go/internal/ffi/lib/ferric.h",
        )
        .expect("CI must reject drift in both committed FFI headers");
    assert!(
        build_position < check_position,
        "CI must generate headers before checking them for drift"
    );
}

#[test]
fn ci_runs_mixed_language_thread_sanitizer_harness() {
    let workflow = read_ci_workflow();
    assert!(
        workflow.contains("FFI Diagnostics (ThreadSanitizer)"),
        "CI must contain the raw-engine diagnostic TSan job"
    );
    assert!(
        workflow.contains("just ffi-tsan-harness"),
        "CI must run the mixed Rust/C TSan harness"
    );
}

#[test]
fn ci_runs_pinned_async_completion_race_under_thread_sanitizer() {
    let workflow = read_ci_workflow();
    assert!(workflow.contains("Pinned Async Completion (ThreadSanitizer)"));
    assert!(workflow.contains("just pinned-async-tsan"));

    let script = read_pinned_async_tsan_harness();
    for required in [
        "-Zsanitizer=thread",
        "-Zbuild-std=std,panic_unwind",
        "cancellation_racing_operation_panic_finalizes_once",
        "--exact",
        "test result: ok. 1 passed;",
    ] {
        assert!(
            script.contains(required),
            "pinned async TSan harness is missing required coverage: {required}"
        );
    }
}

#[test]
fn ci_runs_debug_and_release_panic_containment_harness() {
    let workflow = read_ci_workflow();
    assert!(workflow.contains("FFI Panic Containment"));
    assert!(workflow.contains("just ffi-panic-harness"));

    let script = read_panic_harness();
    assert!(script.contains("for profile in ffi-dev ffi-release"));
    assert!(script.contains("FERRIC_FFI_TEST_PANIC_INJECTION_BUILD=1"));
    assert!(script.contains("--features serde"));
    assert!(script.contains("panic_containment.c"));
    assert!(script.contains("expected 100 header exports"));
}

#[test]
fn tsan_harness_instruments_rust_std_and_c() {
    let script = read_tsan_harness();
    for required in [
        "-Zsanitizer=thread",
        "-Zexternal-clangrt",
        "-Zbuild-std=std,panic_unwind",
        "--crate-type staticlib",
        "-fsanitize=thread",
        "nm -u",
    ] {
        assert!(
            script.contains(required),
            "TSan harness is missing required mixed-language instrumentation: {required}"
        );
    }
}

#[test]
fn every_authored_c_export_uses_the_generated_boundary_wrapper() {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_dir = crate_dir.join("src");
    let mut exports = Vec::new();

    for file in ["engine.rs", "error.rs", "pinned.rs", "types.rs"] {
        let source = std::fs::read_to_string(source_dir.join(file))
            .unwrap_or_else(|_| panic!("could not read FFI source file {file}"));
        let lines: Vec<_> = source.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            let Some(function_suffix) = line.split("extern \"C\" fn ferric_").nth(1) else {
                continue;
            };
            let function_name = format!(
                "ferric_{}",
                function_suffix
                    .split('(')
                    .next()
                    .expect("export name must end before arguments")
            );
            let attributes = lines[index.saturating_sub(6)..index].join("\n");
            assert!(
                attributes.contains("cfg_attr(ferric_ffi_compile, ffi_export"),
                "{function_name} bypasses the generated panic wrapper"
            );
            assert!(
                attributes.contains("#[no_mangle]"),
                "{function_name} is missing its authored export marker"
            );
            exports.push(function_name);
        }
    }

    exports.sort();
    exports.dedup();
    assert_eq!(
        exports.len(),
        100,
        "the export audit count changed; verify every new return category has a panic sentinel"
    );
}

#[test]
fn header_documents_panic_containment_and_sentinels() {
    let header = read_committed_header();
    for required in [
        "PANIC CONTAINMENT",
        "generated wrapper around a non-extern",
        "FERRIC_ERROR_INTERNAL_ERROR",
        "FerricValue: Void",
        "integer/count: 0",
        "allocator abort/OOM",
    ] {
        assert!(
            header.contains(required),
            "panic contract is missing from ferric.h: {required}"
        );
    }
}

#[test]
fn header_contains_ferric_error_enum() {
    let header = read_committed_header();
    assert!(header.contains("FerricError"), "Missing FerricError type");
    // Enum variants should be present with the expected prefix
    assert!(
        header.contains("FERRIC_ERROR_OK"),
        "Missing FERRIC_ERROR_OK variant"
    );
    assert!(
        header.contains("FERRIC_ERROR_NULL_POINTER"),
        "Missing FERRIC_ERROR_NULL_POINTER variant"
    );
    assert!(
        header.contains("FERRIC_ERROR_THREAD_VIOLATION"),
        "Missing FERRIC_ERROR_THREAD_VIOLATION variant"
    );
    assert!(
        header.contains("FERRIC_ERROR_NOT_FOUND"),
        "Missing FERRIC_ERROR_NOT_FOUND variant"
    );
    assert!(
        header.contains("FERRIC_ERROR_PARSE_ERROR"),
        "Missing FERRIC_ERROR_PARSE_ERROR variant"
    );
    assert!(
        header.contains("FERRIC_ERROR_BUFFER_TOO_SMALL"),
        "Missing FERRIC_ERROR_BUFFER_TOO_SMALL variant"
    );
    assert!(
        header.contains("FERRIC_ERROR_PINNED_REENTRANT_CALL"),
        "Missing FERRIC_ERROR_PINNED_REENTRANT_CALL variant"
    );
}

#[test]
fn header_contains_ferric_value_type_enum() {
    let header = read_committed_header();
    assert!(
        header.contains("FerricValueType"),
        "Missing FerricValueType type"
    );
    assert!(
        header.contains("FERRIC_VALUE_TYPE_VOID"),
        "Missing FERRIC_VALUE_TYPE_VOID variant"
    );
    assert!(
        header.contains("FERRIC_VALUE_TYPE_INTEGER"),
        "Missing FERRIC_VALUE_TYPE_INTEGER variant"
    );
    assert!(
        header.contains("FERRIC_VALUE_TYPE_FLOAT"),
        "Missing FERRIC_VALUE_TYPE_FLOAT variant"
    );
    assert!(
        header.contains("FERRIC_VALUE_TYPE_SYMBOL"),
        "Missing FERRIC_VALUE_TYPE_SYMBOL variant"
    );
    assert!(
        header.contains("FERRIC_VALUE_TYPE_MULTIFIELD"),
        "Missing FERRIC_VALUE_TYPE_MULTIFIELD variant"
    );
    assert!(
        header.contains("FERRIC_VALUE_TYPE_EXTERNAL_ADDRESS"),
        "Missing FERRIC_VALUE_TYPE_EXTERNAL_ADDRESS variant"
    );
}

#[test]
fn header_contains_config_types() {
    let header = read_committed_header();
    assert!(
        header.contains("FerricStringEncoding"),
        "Missing FerricStringEncoding type"
    );
    assert!(
        header.contains("FerricConflictStrategy"),
        "Missing FerricConflictStrategy type"
    );
    assert!(header.contains("FerricConfig"), "Missing FerricConfig type");
    assert!(
        header.contains("string_encoding"),
        "FerricConfig is missing string_encoding field"
    );
    assert!(
        header.contains("max_call_depth"),
        "FerricConfig is missing max_call_depth field"
    );
}

#[test]
fn header_contains_ferric_value_struct() {
    let header = read_committed_header();
    assert!(header.contains("FerricValue"), "Missing FerricValue struct");
    assert!(
        header.contains("value_type"),
        "FerricValue is missing value_type field"
    );
    assert!(
        header.contains("string_ptr"),
        "FerricValue is missing string_ptr field"
    );
    assert!(
        header.contains("multifield_ptr"),
        "FerricValue is missing multifield_ptr field"
    );
    assert!(
        header.contains("multifield_len"),
        "FerricValue is missing multifield_len field"
    );
    assert!(
        header.contains("external_pointer"),
        "FerricValue is missing external_pointer field"
    );
}

#[test]
fn header_value_type_is_fixed_width_integer() {
    let header = read_committed_header();
    assert!(
        header.contains("uint32_t value_type;"),
        "FerricValue.value_type must cross the ABI as uint32_t, not a C enum"
    );
    assert!(
        !header.contains("enum FerricValueType value_type;"),
        "FerricValue.value_type must not be typed as a C enum"
    );
}

#[test]
fn header_has_abi_static_assertions() {
    let header = read_committed_header();
    assert!(
        header.contains("ABI STATIC ASSERTIONS"),
        "Missing ABI STATIC ASSERTIONS section"
    );
    assert!(
        header.contains("#define FERRIC_STATIC_ASSERT(COND, MSG)"),
        "Missing FERRIC_STATIC_ASSERT macro definition"
    );
    assert!(
        header.contains("#if defined(__cplusplus) && __cplusplus >= 201103L"),
        "C++ static_assert must be gated on C++11 so older modes use the typedef fallback"
    );
    // Width locks for every caller-populated discriminant field.
    for field in [
        "((FerricValue *)0)->value_type",
        "((FerricValue *)0)->external_type_id",
        "((FerricConfig *)0)->string_encoding",
        "((FerricConfig *)0)->strategy",
        "((FerricPinnedEngineOptions *)0)->autorelease_policy",
    ] {
        assert!(
            header.contains(&format!("FERRIC_STATIC_ASSERT(sizeof({field}) == 4")),
            "Missing width assertion for {field}"
        );
    }
    // Enum object widths for every enum crossing the ABI (rejects
    // -fshort-enums / packed-enum consumer ABIs at compile time).
    for e in [
        "FerricError",
        "FerricValueType",
        "FerricStringEncoding",
        "FerricConflictStrategy",
        "FerricFactType",
        "FerricHaltReason",
        "FerricPinnedAutoreleasePolicy",
        "FerricSerializationFormat",
    ] {
        assert!(
            header.contains(&format!("FERRIC_STATIC_ASSERT(sizeof(enum {e}) == 4")),
            "Missing enum-width assertion for enum {e}"
        );
    }
    // Documented numeric values for the ABI enums (spot-check boundaries).
    for assertion in [
        "FERRIC_STATIC_ASSERT(FERRIC_VALUE_TYPE_VOID == 0",
        "FERRIC_STATIC_ASSERT(FERRIC_VALUE_TYPE_EXTERNAL_ADDRESS == 6",
        "FERRIC_STATIC_ASSERT(FERRIC_ERROR_INVALID_ARGUMENT == 9",
        "FERRIC_STATIC_ASSERT(FERRIC_ERROR_INTERNAL_ERROR == 99",
        "FERRIC_STATIC_ASSERT(FERRIC_STRING_ENCODING_ASCII == 0",
        "FERRIC_STATIC_ASSERT(FERRIC_CONFLICT_STRATEGY_MEA == 3",
        "FERRIC_STATIC_ASSERT(FERRIC_HALT_REASON_ACTION_ERROR == 3",
        "FERRIC_STATIC_ASSERT(FERRIC_SERIALIZATION_FORMAT_POSTCARD == 4",
    ] {
        assert!(
            header.contains(assertion),
            "Missing numeric-value assertion: {assertion}"
        );
    }
}

#[test]
fn header_enums_have_no_trailing_commas() {
    // Strict C++98/03 (-pedantic-errors) rejects a trailing comma after the
    // last enum variant; the generator strips the ones cbindgen emits.
    let header = read_committed_header();
    assert!(
        !header.contains(",\n}"),
        "committed header must not contain trailing commas before closing braces"
    );
}

#[test]
fn header_contains_ferric_engine_opaque() {
    let header = read_committed_header();
    // FerricEngine must appear as an opaque struct, not with its fields exposed
    assert!(
        header.contains("FerricEngine"),
        "Missing FerricEngine forward declaration"
    );
    // The engine's Rust-side fields (engine, error_state) must NOT appear
    assert!(
        !header.contains("error_state"),
        "FerricEngine internal field 'error_state' must not appear in the header"
    );
}

#[test]
fn header_contains_engine_lifecycle_functions() {
    let header = read_committed_header();
    assert!(
        header.contains("ferric_engine_new"),
        "Missing ferric_engine_new"
    );
    assert!(
        header.contains("ferric_engine_new_with_config"),
        "Missing ferric_engine_new_with_config"
    );
    assert!(
        header.contains("ferric_engine_free"),
        "Missing ferric_engine_free"
    );
    assert!(
        header.contains("ferric_engine_load_string"),
        "Missing ferric_engine_load_string"
    );
    assert!(
        header.contains("ferric_engine_reset"),
        "Missing ferric_engine_reset"
    );
    assert!(
        header.contains("ferric_engine_free_unchecked"),
        "Missing ferric_engine_free_unchecked"
    );
}

#[test]
fn header_contains_execution_functions() {
    let header = read_committed_header();
    assert!(
        header.contains("ferric_engine_run"),
        "Missing ferric_engine_run"
    );
    assert!(
        header.contains("ferric_engine_step"),
        "Missing ferric_engine_step"
    );
    assert!(
        header.contains("ferric_engine_assert_string"),
        "Missing ferric_engine_assert_string"
    );
    assert!(
        header.contains("ferric_engine_retract"),
        "Missing ferric_engine_retract"
    );
    assert!(
        header.contains("ferric_engine_assert_template"),
        "Missing ferric_engine_assert_template"
    );
    assert!(
        header.contains("ferric_engine_get_fact_slot_by_name"),
        "Missing ferric_engine_get_fact_slot_by_name"
    );
}

#[test]
fn header_contains_query_functions() {
    let header = read_committed_header();
    assert!(
        header.contains("ferric_engine_action_diagnostic_count"),
        "Missing ferric_engine_action_diagnostic_count"
    );
    assert!(
        header.contains("ferric_engine_action_diagnostic_copy"),
        "Missing ferric_engine_action_diagnostic_copy"
    );
    assert!(
        header.contains("ferric_engine_clear_action_diagnostics"),
        "Missing ferric_engine_clear_action_diagnostics"
    );
    assert!(
        header.contains("ferric_engine_fact_count"),
        "Missing ferric_engine_fact_count"
    );
    assert!(
        header.contains("ferric_engine_get_fact_field_count"),
        "Missing ferric_engine_get_fact_field_count"
    );
    assert!(
        header.contains("ferric_engine_get_fact_field"),
        "Missing ferric_engine_get_fact_field"
    );
    assert!(
        header.contains("ferric_engine_get_global"),
        "Missing ferric_engine_get_global"
    );
    assert!(
        header.contains("ferric_engine_get_output"),
        "Missing ferric_engine_get_output"
    );
    assert!(
        header.contains("ferric_engine_get_output_copy"),
        "Missing ferric_engine_get_output_copy"
    );
}

#[test]
fn header_contains_error_functions() {
    let header = read_committed_header();
    assert!(
        header.contains("ferric_last_error_global"),
        "Missing ferric_last_error_global"
    );
    assert!(
        header.contains("ferric_clear_error_global"),
        "Missing ferric_clear_error_global"
    );
    assert!(
        header.contains("ferric_last_error_global_copy"),
        "Missing ferric_last_error_global_copy"
    );
    assert!(
        header.contains("ferric_engine_last_error"),
        "Missing ferric_engine_last_error"
    );
    assert!(
        header.contains("ferric_engine_last_error_copy"),
        "Missing ferric_engine_last_error_copy"
    );
    assert!(
        header.contains("ferric_pinned_engine_last_error_copy"),
        "Missing ferric_pinned_engine_last_error_copy"
    );
    assert!(
        header.contains("ferric_engine_clear_error"),
        "Missing ferric_engine_clear_error"
    );
}

#[test]
fn header_contains_value_free_functions() {
    let header = read_committed_header();
    assert!(
        header.contains("ferric_string_free"),
        "Missing ferric_string_free"
    );
    // Both value-free functions report invalid discriminants via FerricError.
    assert!(
        header.contains("enum FerricError ferric_value_free("),
        "ferric_value_free must return FerricError"
    );
    assert!(
        header.contains("enum FerricError ferric_value_array_free("),
        "ferric_value_array_free must return FerricError"
    );
}

#[test]
fn header_contains_multifield_copy_contract() {
    let header = read_committed_header();
    assert!(
        header.contains("enum FerricError ferric_value_multifield_copy("),
        "Missing ferric_value_multifield_copy declaration"
    );
    assert!(
        header.contains("*elements FERRIC_COUNTED_BY(len),"),
        "multifield copy input must be annotated with FERRIC_COUNTED_BY(len)"
    );
    for required in [
        "complete nested value tree are borrowed",
        "Ferric never retains or frees caller-provided",
        "External-address payload pointers are copied shallowly",
        "remains Void on every failure",
        "not overlap the borrowed input tree",
        "foreign-allocated value trees must not be passed",
    ] {
        assert!(
            header.contains(required),
            "multifield ownership contract is missing: {required}"
        );
    }
}

// ── Bounds-safety annotation tests ─────────────────────────────────────

#[test]
fn header_has_bounds_safety_macros() {
    let header = read_committed_header();
    assert!(
        header.contains("#define FERRIC_COUNTED_BY(N)"),
        "Missing FERRIC_COUNTED_BY macro definition"
    );
    assert!(
        header.contains("#define FERRIC_SIZED_BY(N)"),
        "Missing FERRIC_SIZED_BY macro definition"
    );
    assert!(
        header.contains("#define FERRIC_NULL_TERMINATED"),
        "Missing FERRIC_NULL_TERMINATED macro definition"
    );
    assert!(
        header.contains("__has_feature(bounds_safety)"),
        "Missing bounds_safety feature detection"
    );
}

#[test]
fn header_has_bounds_safety_escape_hatch() {
    let header = read_committed_header();
    assert!(
        header.contains("FERRIC_NO_BOUNDS_ANNOTATIONS"),
        "Missing FERRIC_NO_BOUNDS_ANNOTATIONS escape hatch"
    );
}

#[test]
fn header_has_counted_by_and_sized_by_annotations() {
    let header = read_committed_header();

    // Struct field: FerricValue.multifield_ptr counted_by multifield_len
    assert!(
        header.contains("*multifield_ptr FERRIC_COUNTED_BY(multifield_len)"),
        "Missing FERRIC_COUNTED_BY on FerricValue.multifield_ptr"
    );

    // ferric_value_array_free: arr counted_by len
    assert!(
        header.contains("*arr FERRIC_COUNTED_BY(len)"),
        "Missing FERRIC_COUNTED_BY on ferric_value_array_free arr parameter"
    );

    // ferric_value_multifield_copy: elements counted_by len
    assert!(
        header.contains("*elements FERRIC_COUNTED_BY(len)"),
        "Missing FERRIC_COUNTED_BY on ferric_value_multifield_copy elements"
    );
    assert!(
        header.contains("ferric_value_symbol_bytes(const uint8_t *data FERRIC_SIZED_BY(len),"),
        "Missing FERRIC_SIZED_BY on ferric_value_symbol_bytes data"
    );
    assert!(
        header.contains("ferric_value_string_bytes(const uint8_t *data FERRIC_SIZED_BY(len),"),
        "Missing FERRIC_SIZED_BY on ferric_value_string_bytes data"
    );

    // ferric_last_error_global_copy: buf sized_by buf_len
    assert!(
        header.contains("ferric_last_error_global_copy(char *buf FERRIC_SIZED_BY(buf_len)"),
        "Missing FERRIC_SIZED_BY on ferric_last_error_global_copy buf parameter"
    );

    // ferric_engine_last_error_copy: buf sized_by buf_len
    assert!(
        header.contains("*buf FERRIC_SIZED_BY(buf_len),\n                                               uintptr_t buf_len,\n                                               uintptr_t *out_len);"),
        "Missing FERRIC_SIZED_BY on ferric_engine_last_error_copy buf parameter"
    );

    // ferric_pinned_engine_last_error_copy: buf sized_by buf_len
    assert!(
        header.contains("ferric_pinned_engine_last_error_copy(const struct FerricPinnedEngine *engine,\n                                                      char *buf FERRIC_SIZED_BY(buf_len),"),
        "Missing FERRIC_SIZED_BY on ferric_pinned_engine_last_error_copy buf parameter"
    );

    // ferric_engine_action_diagnostic_copy: buf sized_by buf_len
    assert!(
        header.contains("*buf FERRIC_SIZED_BY(buf_len),\n                                                      uintptr_t buf_len,\n                                                      uintptr_t *out_len);"),
        "Missing FERRIC_SIZED_BY on ferric_engine_action_diagnostic_copy buf parameter"
    );
    assert!(
        header.contains("ferric_engine_get_output_copy(const struct FerricEngine *engine,\n                                               const char * FERRIC_NULL_TERMINATED channel,\n                                               char *buf FERRIC_SIZED_BY(buf_len),"),
        "Missing bounds annotations on ferric_engine_get_output_copy"
    );
}

#[test]
fn header_documents_embedded_nul_policy() {
    let header = read_committed_header();
    assert!(header.contains("Embedded NUL policy:"));
    assert!(header.contains("ferric_value_symbol_bytes()"));
    assert!(header.contains("ferric_value_string_bytes()"));
    assert!(header.contains("Legacy FerricValue"));
    assert!(header.contains("ferric_engine_get_output_copy()"));
}

#[test]
fn header_has_null_terminated_annotations() {
    let header = read_committed_header();

    // Struct field: FerricValue.string_ptr
    assert!(
        header.contains("FERRIC_NULL_TERMINATED string_ptr"),
        "Missing FERRIC_NULL_TERMINATED on FerricValue.string_ptr"
    );

    // Return types
    assert!(
        header.contains("char * FERRIC_NULL_TERMINATED ferric_engine_last_error("),
        "Missing FERRIC_NULL_TERMINATED on ferric_engine_last_error return type"
    );
    assert!(
        header.contains("char * FERRIC_NULL_TERMINATED ferric_engine_get_output("),
        "Missing FERRIC_NULL_TERMINATED on ferric_engine_get_output return type"
    );
    assert!(
        header.contains("char * FERRIC_NULL_TERMINATED ferric_last_error_global("),
        "Missing FERRIC_NULL_TERMINATED on ferric_last_error_global return type"
    );

    // String parameters
    assert!(
        header.contains("FERRIC_NULL_TERMINATED source);"),
        "Missing FERRIC_NULL_TERMINATED on load_string source parameter"
    );
    assert!(
        header.contains("FERRIC_NULL_TERMINATED source,"),
        "Missing FERRIC_NULL_TERMINATED on assert_string source parameter"
    );
    assert!(
        header.contains("FERRIC_NULL_TERMINATED channel);"),
        "Missing FERRIC_NULL_TERMINATED on get_output channel parameter"
    );
    assert!(
        header.contains("FERRIC_NULL_TERMINATED name,"),
        "Missing FERRIC_NULL_TERMINATED on get_global name parameter"
    );

    // ferric_string_free: ptr
    assert!(
        header.contains("FERRIC_NULL_TERMINATED ptr)"),
        "Missing FERRIC_NULL_TERMINATED on ferric_string_free ptr parameter"
    );
}
