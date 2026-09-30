//! Regressions for the CLIPS compatibility fixes (#320-#346) that inspect
//! stored values, diagnostics, limits and evaluation order through the public
//! API. The CLIPS-verified programs themselves live in
//! `tests/clips_compat/corpus`. One target keeps this a single test binary.

mod callable_iterator_validation;
mod callable_local_scope;
mod create_void_fields;
mod expression_query_scope;
mod implode;
mod member_subsequences;
mod multifield_printout;
mod nth_indices;
mod numeric_comparisons;
mod numeric_extrema;
#[cfg(feature = "serde")]
mod query_chronology_snapshot;
mod query_mutation_budget;
mod query_run_boundaries;
mod rounding;
mod string_index;
mod string_length;
mod substring_bounds;
