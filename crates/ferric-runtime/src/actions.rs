//! RHS action execution for rule firings.
//!
//! When a rule fires, its right-hand side (RHS) contains actions to execute.
//! Phase 2 supports a core subset of CLIPS RHS actions:
//!
//! - `assert`: Create a new fact in working memory.
//! - `retract`: Remove a fact from working memory.
//! - `modify`: Retract and re-assert a fact with modified slot values.
//! - `duplicate`: Assert a copy of a fact with modified slot values.
//!
//! ## Phase 2 implementation plan
//!
//! - Pass 009: Action execution (assert, retract, modify, duplicate)

// Implementation will be added in Pass 009.

#[cfg(test)]
mod tests {
    // Action execution tests will be added as the implementation lands.
    //
    // Planned test areas:
    // - assert action creates fact and triggers Rete propagation
    // - retract action removes fact and triggers Rete retraction
    // - modify action performs retract + assert with changed slots
    // - duplicate action asserts new fact with changed slots (original unchanged)
    // - variable binding resolution in action arguments
    // - error handling for invalid actions (e.g., retract non-existent fact)
    //
    // Planned property tests:
    // - modify is equivalent to retract + assert with same slots
    // - duplicate does not affect the original fact
}
