//! Negative node: blocks parent tokens when a matching fact exists.
//!
//! A negative node acts like a join node but with inverted semantics: when a
//! fact matches the pattern, it *blocks* the parent token from propagating
//! downstream (rather than extending the partial match). When the blocking
//! fact is retracted, the parent token becomes unblocked and propagates.
//!
//! ## Blocker tracking
//!
//! Each negative node maintains a map from parent tokens to their set of
//! blocker facts. A parent token with an empty blocker set propagates; one
//! with any blockers does not.
//!
//! ## Phase 2 implementation plan
//!
//! - Pass 006: Negative node (single-pattern) and blocker tracking
//! - Pass 010: NCC and exists nodes extend this foundation

// Implementation will be added in Pass 006.

#[cfg(test)]
mod tests {
    // Negative node tests will be added as the implementation lands.
    //
    // Planned test areas:
    // - single-pattern negation: (not (person ?x))
    // - blocker tracking: assert fact blocks, retract fact unblocks
    // - multiple blockers per parent token
    // - retraction cleanup: no orphaned blocker entries
    // - consistency checks after assert/retract cycles
    // - interaction with positive patterns in same rule
    //
    // Planned property tests:
    // - assert then retract always returns to pre-assert state
    // - blocker count never goes negative
    // - no downstream activations while any blocker exists
}
