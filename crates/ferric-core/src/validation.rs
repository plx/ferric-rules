//! Pattern restriction validation (compile-time).
//!
//! Validates pattern restrictions at compile time according to Section 7.7
//! of the CLIPS specification. Reports stable error codes `E0001`–`E0005`
//! with source spans for diagnostics.
//!
//! ## Phase 2 implementation plan
//!
//! - Pass 011: Pattern validation and source-located compile errors

// Implementation will be added in Pass 011.

#[cfg(test)]
mod tests {
    // Validation tests will be added as the implementation lands.
    //
    // Planned test areas:
    // - E0001–E0005 error codes for each restriction violation
    // - source span preservation through validation errors
    // - classic vs strict mode severity differences
    // - valid patterns pass without diagnostics
    //
    // Planned property tests:
    // - validation is deterministic (same input always produces same errors)
    // - valid patterns never produce errors
}
