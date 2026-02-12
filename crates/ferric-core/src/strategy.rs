//! Conflict resolution strategies for the agenda.
//!
//! CLIPS supports four conflict resolution strategies that determine the order
//! in which activations are fired:
//!
//! - **Depth** (default): Higher salience first, then most recent first.
//! - **Breadth**: Higher salience first, then least recent first.
//! - **LEX**: Higher salience first, then lexicographic comparison of fact
//!   recency (most recent fact in token compared first).
//! - **MEA**: Higher salience first, then the recency of the first pattern's
//!   fact determines order.
//!
//! Phase 1 implements depth only. Phase 2 adds all four strategies with
//! stable total ordering.
//!
//! ## Phase 2 implementation plan
//!
//! - Pass 007: Agenda conflict strategies and ordering contract

// Implementation will be added in Pass 007.

#[cfg(test)]
mod tests {
    // Strategy ordering tests will be added as the implementation lands.
    //
    // Planned test areas:
    // - depth ordering: salience > recency (most recent first) > sequence
    // - breadth ordering: salience > recency (least recent first) > sequence
    // - LEX ordering: salience > lexicographic recency comparison
    // - MEA ordering: salience > first-pattern recency > LEX tiebreak
    // - strategy switching at runtime
    // - total ordering: no ties (all strategies produce deterministic order)
    //
    // Planned property tests:
    // - ordering is always a total order (antisymmetric, transitive, total)
    // - pop always returns the highest-priority activation
    // - adding an activation preserves ordering of existing activations
}
