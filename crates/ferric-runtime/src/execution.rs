//! Engine execution loop: run, step, halt, and reset.
//!
//! The execution loop pops activations from the agenda and fires them,
//! executing RHS actions that may assert/retract/modify facts and trigger
//! further Rete propagation.
//!
//! ## Phase 2 implementation plan
//!
//! - Pass 008: Run/step/halt/reset execution loop
//! - Pass 009: Action execution (assert, retract, modify, duplicate)

// Implementation will be added in Pass 008/009.

#[cfg(test)]
mod tests {
    // Execution loop tests will be added as the implementation lands.
    //
    // Planned test areas:
    // - run to completion (agenda exhaustion)
    // - run with step limit
    // - step fires exactly one activation
    // - halt stops execution mid-run
    // - reset clears working memory and re-asserts deffacts
    // - run/assert/retract interaction (facts asserted by RHS trigger new activations)
    //
    // Planned property tests:
    // - run never fires more activations than agenda size at start + generated
    // - step is idempotent on empty agenda (returns immediately)
    // - halt flag is respected within one step boundary
}
