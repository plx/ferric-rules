//! Kani proof harnesses for `validation` (see docs/verification.md).
//!
//! This module defines the diagnostics types; the validators that emit them
//! live in `compiler.rs` (E0005, see compiler/kani_proofs.rs) and in the
//! runtime loader (E0001, E0002, E0003 and E0005, modeled in
//! verification/verus/pattern_restrictions.rs). No validator in the
//! workspace constructs E0004 (`ForallUnboundVariable`); the harnesses below
//! check only the code that variant would carry.
//!
//! What is proved, on the production code (the expected codes are written
//! here from the variant documentation, validation.rs:89-109):
//!
//! * `violation_codes_are_the_documented_stable_codes`: each of the five
//!   violation kinds (with symbolic depth and limit for `NestingTooDeep`)
//!   reports its documented code E0001-E0005, and the codes are pairwise
//!   distinct, so a code identifies its kind.
//! * `validation_error_records_kind_code_location_and_stage`: for the four
//!   kinds without numeric payload, `PatternValidationError::new` with any
//!   location (or none) and either stage stores the kind, its documented
//!   code, that location and stage, and a non-empty suggestion.
//!
//! Bounds: string payloads are empty; `NestingTooDeep`'s suggestion
//! (formatting a symbolic `usize`) is not exercised by the second harness.
//!
//! Not covered: the `Display` implementations (string formatting, covered by
//! the unit and property tests in this module).

use super::*;

/// The stable codes, written from the variant documentation
/// (validation.rs:89-109): E0001 nesting depth, E0002 forall condition not
/// a single pattern, E0003 nested forall, E0004 unbound forall variable,
/// E0005 unsupported nesting combination.
fn documented_code(variant: u8) -> &'static [u8; 5] {
    match variant {
        0 => b"E0001",
        1 => b"E0002",
        2 => b"E0003",
        3 => b"E0004",
        _ => b"E0005",
    }
}

/// One violation of each documented kind, with symbolic payload where the
/// payload is plain data.
fn violation(variant: u8) -> PatternViolation {
    match variant {
        0 => PatternViolation::NestingTooDeep {
            depth: kani::any(),
            max: kani::any(),
        },
        1 => PatternViolation::ForallConditionNotSinglePattern,
        2 => PatternViolation::NestedForall,
        3 => PatternViolation::ForallUnboundVariable {
            var_name: String::new(),
        },
        _ => PatternViolation::UnsupportedNestingCombination {
            description: String::new(),
        },
    }
}

#[kani::proof]
#[kani::unwind(7)]
fn violation_codes_are_the_documented_stable_codes() {
    for variant in 0..5_u8 {
        let kind = violation(variant);
        let code = kind.code();
        assert_eq!(code.as_bytes(), documented_code(variant));
        // Codes are pairwise distinct, so a code identifies its violation kind.
        for other in 0..5_u8 {
            if other != variant {
                assert_ne!(code.as_bytes(), documented_code(other));
            }
        }
        std::mem::forget(kind);
    }
}

#[kani::proof]
#[kani::unwind(7)]
fn validation_error_records_kind_code_location_and_stage() {
    // The kinds whose suggestion is a fixed text (`NestingTooDeep` and
    // `ForallUnboundVariable` format theirs).
    for variant in [1_u8, 2, 4] {
        let kind = violation(variant);
        let location = if kani::any() {
            Some(SourceLocation::new(
                kani::any(),
                kani::any(),
                kani::any(),
                kani::any(),
            ))
        } else {
            None
        };
        let stage = if kani::any() {
            ValidationStage::AstInterpretation
        } else {
            ValidationStage::ReteCompilation
        };
        let error = PatternValidationError::new(kind.clone(), location, stage);
        assert_eq!(error.code.as_bytes(), documented_code(variant));
        assert!(error.kind == kind);
        assert_eq!(error.location, location);
        assert_eq!(error.stage, stage);
        assert!(error
            .suggestion
            .as_ref()
            .is_some_and(|text| !text.is_empty()));
        std::mem::forget(error);
        std::mem::forget(kind);
    }
}
