//! Exists node: existential quantification over patterns.
//!
//! An exists node handles `(exists (P1 P2 ...))` by maintaining a support
//! count. When the support count transitions from zero to non-zero, the
//! parent token propagates. When it transitions back to zero, the propagation
//! is retracted.
//!
//! ## Phase 2 implementation plan
//!
//! - Pass 010: NCC and exists nodes and cleanup invariants

// Implementation will be added in Pass 010.

#[cfg(test)]
mod tests {
    // Exists node tests will be added as the implementation lands.
    //
    // Planned test areas:
    // - single-pattern exists: (exists (person ?x))
    // - support counting: multiple supporting facts
    // - retraction removes support; zero-support retracts downstream
    // - cleanup: no orphaned support entries
    //
    // Planned property tests:
    // - support count is always non-negative
    // - support transitions 0→N and N→0 are symmetric
}
