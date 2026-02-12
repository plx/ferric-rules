//! NCC (Negated Conjunctive Condition) node: negates a conjunction of patterns.
//!
//! An NCC node handles `(not (and P1 P2 ...))` by maintaining a partner
//! sub-network that evaluates the conjunction. When the conjunction has any
//! complete matches, the parent token is blocked; when all matches are
//! retracted, the parent token propagates.
//!
//! ## Phase 2 implementation plan
//!
//! - Pass 010: NCC and exists nodes and cleanup invariants

// Implementation will be added in Pass 010.

#[cfg(test)]
mod tests {
    // NCC node tests will be added as the implementation lands.
    //
    // Planned test areas:
    // - two-pattern conjunction negation: (not (and (a ?x) (b ?x)))
    // - partner sub-network cleanup on retraction
    // - NCC with shared alpha memories
    // - consistency checks across partner/result memories
    //
    // Planned property tests:
    // - conjunction match count tracking is symmetric under assert/retract
    // - no orphaned tokens in partner sub-network after full retraction
}
