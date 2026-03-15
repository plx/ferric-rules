//! Header drift detection and smoke tests (Pass 008).
//!
//! These tests verify that the committed `ferric.h` exists and contains all
//! expected symbols, banners, and include guards.
//!
//! For full drift detection, run `cargo build -p ferric-ffi` and then check
//! `git diff --exit-code crates/ferric-ffi/ferric.h` in CI.

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
             Run `cargo build -p ferric-ffi` to generate it."
        )
    })
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
    assert!(
        header.contains("ferric_value_free"),
        "Missing ferric_value_free"
    );
    assert!(
        header.contains("ferric_value_array_free"),
        "Missing ferric_value_array_free"
    );
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

    // ferric_engine_action_diagnostic_copy: buf sized_by buf_len
    assert!(
        header.contains("*buf FERRIC_SIZED_BY(buf_len),\n                                                      uintptr_t buf_len,\n                                                      uintptr_t *out_len);"),
        "Missing FERRIC_SIZED_BY on ferric_engine_action_diagnostic_copy buf parameter"
    );
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
