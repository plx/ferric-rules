//! Ferric-specific behavior around the CLIPS compatibility fixes (#320-#346)
//! that the CLIPS-verified corpus in `tests/clips_compat/corpus` cannot
//! express: action-loop budgets, snapshots, load-time validation, callable
//! frames after errors, run boundaries, and Ferric's own output actions. One
//! target keeps this a single test binary.

mod callable_iterator_validation;
mod callable_local_scope;
mod expression_query_scope;
mod output_actions;
#[cfg(feature = "serde")]
mod query_chronology_snapshot;
mod query_mutation_budget;
mod query_run_boundaries;
