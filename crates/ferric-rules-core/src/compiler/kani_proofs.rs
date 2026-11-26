//! Kani proof harnesses for `compiler` (see docs/verification.md).
//!
//! What is proved, on the production code. Expected results are written in
//! this file from the documented limits (`MAX_ALPHA_TESTS`: "Maximum value
//! tests in one alpha path, plus at most one ordered field-count test") and
//! diagnostics (structural errors are the E0005 `UnsupportedNestingCombination`
//! of Rete compilation), not copied from `compiler.rs`:
//!
//! * `check_limit_is_exact`: for every demand and limit, `check_limit`
//!   succeeds exactly when the demand is within the limit, and otherwise
//!   reports that resource with the demand and the limit.
//! * `ensure_non_empty_is_exact`: `EmptyRule` exactly for an empty list
//!   (lists of up to three items).
//! * `alpha_test_count_admits_at_most_one_field_count_test`: zero to three
//!   ordered field-count tests (symbolic bounds and slots) are accepted
//!   exactly when there is at most one; otherwise the error names the
//!   field-count resource with the exact count and limit 1.
//! * `alpha_test_count_accepts_top_level_sequence_tests`: zero to three
//!   whole-fact `Sequence` tests are accepted at the top level of a path.
//! * `sequence_bindings_must_select_logical_fields`: for an ordered sequence
//!   plan of one or two single fields, `validate_pattern_tests` accepts a
//!   pattern exactly when its alpha source is an ordered relation and both
//!   its binding and its negated binding select one of the plan's logical
//!   fields (indexes below 4); otherwise it reports exactly one E0005.
//! * `top_level_predicates_are_accepted`: zero to three predicate conditions
//!   (any indexes) pass `validate_conditions`.
//! * `maximum_new_beta_nodes_counts_ncc_partners`: for zero to three
//!   top-level (empty) NCCs or patterns, the beta-node demand is the
//!   terminal plus two nodes per NCC or one per pattern.
//!
//! Harness technique and bounds: inputs live in stack arrays, every list
//! length is concrete (a loop over the lengths), every list holds a single
//! test or condition variant, payloads (constants, slots, bounds, flags,
//! indexes) are symbolic, and values with recursive drop glue are leaked
//! with `mem::forget` (their drop is not part of any property).
//!
//! Not covered by Kani, with the measured reason (Kani 0.68, CBMC 6.11,
//! 7 GB address-space limit, one harness at a time). The validators keep
//! `&T` pointers in a heap `Vec` worklist and read child vectors back from
//! heap buffers; CBMC's value sets are not index-sensitive inside heap
//! buffers, so a popped pointer may stand for any pushed element, and the
//! `Any`/`Ncc`/pattern arms then read child vectors whose loops never
//! become bounded. Diagnostics also format strings.
//!
//! * Symbolic or mixed shapes: one `constant_test_count` call on a single
//!   test whose variant was a symbolic choice of two ran out of memory after
//!   51 s; a two-test version after 169 s; a 2x4x4 sweep of mixed flat test
//!   variants was still in symbolic execution after 10 min.
//! * Disjunctions: `validate_alpha_test_count` on one empty disjunction ran
//!   out of memory after 208 s.
//! * Nested conditions: `validate_conditions` on `[pattern, ncc[pattern]]`
//!   was still in symbolic execution after 10.5 min (4.7 GB);
//!   `maximum_new_beta_nodes` over NCCs with predicate children ran out of
//!   memory after 404 s.
//! * Patterns and diagnostics: `validate_conditions` on a single pattern ran
//!   out of memory after 355 s and on a single empty NCC after 337 s (the
//!   `format!` context and error construction); `validate_pattern_structure`
//!   with two bindings did not finish in 600 s.
//! * The 64-condition and 64-test boundaries: 64/65/66 predicates ran out of
//!   memory after 487 s and 64/65 equality tests after 359 s (a 65-element
//!   worklist).
//! * Node sharing: `ensure_alpha_path` called twice with one test-free
//!   pattern on an empty `AlphaNetwork` (no `ReteNetwork` involved) ran out
//!   of memory after 350 s (fresh `FxHashMap` insertions for the entry node
//!   and the path cache).
//! * `ensure_join_node`, `compile_rule`, `compile_conditions`,
//!   `install_condition_plan` and `remove_rules` need a `BetaNetwork` or a
//!   `ReteNetwork`, which CBMC cannot build here (measured by the beta and
//!   rete end-to-end proof modules: `BetaNetwork::new`'s single node-map
//!   insertion ran out of memory after 318 s, and `ReteNetwork::new` reaches
//!   `RandomState`'s `getrandom` and ran out of memory after about 420 s), so
//!   "compiled networks pass `validate_consistency`" is not checked here.
//!
//! These properties are proved without bounds on a model in
//! verification/verus/core_compiler.rs: `validate_conditions` accepts exactly
//! the specified condition trees of any shape and size (including the
//! 64-node limit, reported as demand 65, and acceptance implies every NCC is
//! non-empty), variable reuse is reported exactly, `maximum_new_beta_nodes`
//! is exact and bounds every installation, alpha paths are shared exactly
//! when their keys are equal and reused on recompilation, and the snapshot's
//! alpha-cache check accepts every compiled state.

use super::*;
use crate::sequence::{SequenceField, SequenceSegment, SequenceSource};

/// The specification's reading of `CompileError::ResourceLimit`: which limit
/// was hit (identified by the resource name's length, which differs for every
/// resource the compiler names, so no string comparison loop is needed),
/// the demand, and the limit.
fn is_limit(
    result: &Result<(), CompileError>,
    resource: &str,
    required: usize,
    limit: usize,
) -> bool {
    matches!(
        result,
        Err(CompileError::ResourceLimit { resource: r, required: q, limit: l })
            if r.len() == resource.len() && *q == required && *l == limit
    )
}

/// `count` errors, each the documented E0005 diagnostic of Rete compilation.
fn is_e0005_list(result: &Result<(), CompileError>, count: usize) -> bool {
    match result {
        Err(CompileError::Validation(errors)) => {
            errors.len() == count
                && errors.iter().all(|error| {
                    error.code.len() == 5
                        && error.code.as_bytes()[4] == b'5'
                        && matches!(
                            error.kind,
                            PatternViolation::UnsupportedNestingCombination { .. }
                        )
                        && error.location.is_none()
                        && error.stage == ValidationStage::ReteCompilation
                        && error.suggestion.is_some()
                })
        }
        _ => false,
    }
}

fn relation() -> AlphaEntryType {
    AlphaEntryType::OrderedRelation(Symbol(SymbolId::Ascii(0)))
}

fn plain_pattern(negated: bool, exists: bool) -> CompilablePattern {
    CompilablePattern {
        entry_type: relation(),
        constant_tests: Vec::new(),
        sequence: None,
        variable_slots: Vec::new(),
        negated_variable_slots: Vec::new(),
        negated,
        exists,
    }
}

fn field_count_test() -> ConstantTest {
    ConstantTest {
        slot: SlotIndex::Ordered(kani::any()),
        test_type: ConstantTestType::OrderedFieldCount {
            min: kani::any(),
            max: kani::any(),
        },
    }
}

fn empty_sequence_test() -> ConstantTest {
    ConstantTest {
        slot: SlotIndex::Ordered(kani::any()),
        test_type: ConstantTestType::Sequence(Box::new(SequencePattern {
            segments: Vec::new(),
            tests: Vec::new(),
        })),
    }
}

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

#[kani::proof]
#[kani::unwind(3)]
fn check_limit_is_exact() {
    let required: usize = kani::any();
    let limit: usize = kani::any();
    let result = ReteCompiler::check_limit("alpha tests", required, limit);
    assert_eq!(result.is_ok(), required <= limit);
    if required > limit {
        assert!(is_limit(&result, "alpha tests", required, limit));
    }
    std::mem::forget(result);
}

#[kani::proof]
#[kani::unwind(5)]
fn ensure_non_empty_is_exact() {
    let items = [kani::any::<u8>(), kani::any(), kani::any()];
    let n: usize = kani::any_where(|n: &usize| *n <= 3);
    let result = ReteCompiler::ensure_non_empty(&items[..n]);
    assert_eq!(result.is_ok(), n > 0);
    if n == 0 {
        assert_eq!(result, Err(CompileError::EmptyRule));
    }
    kani::cover!(n == 3, "three items");
    kani::cover!(n == 0, "empty");
    std::mem::forget(result);
}

#[kani::proof]
#[kani::unwind(5)]
fn alpha_test_count_admits_at_most_one_field_count_test() {
    for n in 0..4_usize {
        let tests = [field_count_test(), field_count_test(), field_count_test()];
        let result = ReteCompiler::validate_alpha_test_count(&tests[..n]);
        if n <= 1 {
            assert!(result.is_ok());
        } else {
            assert!(is_limit(&result, "ordered field-count tests", n, 1));
        }
        std::mem::forget(result);
        std::mem::forget(tests);
    }
}

#[kani::proof]
#[kani::unwind(5)]
fn alpha_test_count_accepts_top_level_sequence_tests() {
    for n in 0..4_usize {
        let sequences = [
            empty_sequence_test(),
            empty_sequence_test(),
            empty_sequence_test(),
        ];
        let result = ReteCompiler::validate_alpha_test_count(&sequences[..n]);
        assert!(result.is_ok());
        std::mem::forget(result);
        std::mem::forget(sequences);
    }
}

// ---------------------------------------------------------------------------
// Condition validation
// ---------------------------------------------------------------------------

#[kani::proof]
#[kani::unwind(5)]
fn top_level_predicates_are_accepted() {
    for n in 0..4_usize {
        let predicates = [
            CompilableCondition::Predicate {
                condition_index: kani::any(),
            },
            CompilableCondition::Predicate {
                condition_index: kani::any(),
            },
            CompilableCondition::Predicate {
                condition_index: kani::any(),
            },
        ];
        let result = ReteCompiler::validate_conditions(&predicates[..n]);
        assert!(result.is_ok());
        std::mem::forget(result);
        std::mem::forget(predicates);
    }
}

/// An ordered sequence plan of `width` single fields with no tests.
fn ordered_plan(width: usize) -> SequencePattern {
    SequencePattern {
        segments: vec![SequenceSegment {
            source: SequenceSource::Ordered,
            fields: vec![SequenceField::Single; width],
        }],
        tests: Vec::new(),
    }
}

#[kani::proof]
#[kani::unwind(5)]
fn sequence_bindings_must_select_logical_fields() {
    for width in 1..3_usize {
        let positive: usize = kani::any_where(|i: &usize| *i < 4);
        let negated: usize = kani::any_where(|i: &usize| *i < 4);
        let template_entry: bool = kani::any();
        let mut pattern = plain_pattern(false, false);
        pattern.sequence = Some(ordered_plan(width));
        pattern.variable_slots = vec![(SlotIndex::Ordered(positive), Symbol(SymbolId::Ascii(0)))];
        pattern.negated_variable_slots = vec![(
            SlotIndex::Ordered(negated),
            Symbol(SymbolId::Ascii(1)),
            JoinTestType::NotEqual,
        )];
        if template_entry {
            pattern.entry_type = AlphaEntryType::Template(crate::fact::TemplateId::default());
        }
        let result = ReteCompiler::validate_pattern_tests(&pattern);
        // Specification: an ordered projection reads only ordered alpha
        // sources, and every selector names one of its `width` logical fields.
        let expected_ok = !template_entry && positive < width && negated < width;
        if expected_ok {
            assert!(result.is_ok());
        } else {
            assert!(is_e0005_list(&result, 1));
        }
        kani::cover!(expected_ok && width == 2, "accepted two-field plan");
        kani::cover!(!template_entry && positive == width, "first invalid field");
        std::mem::forget(result);
        std::mem::forget(pattern);
    }
}

// ---------------------------------------------------------------------------
// Beta-node demand
// ---------------------------------------------------------------------------

#[kani::proof]
#[kani::unwind(5)]
fn maximum_new_beta_nodes_counts_ncc_partners() {
    for n in 0..4_usize {
        let nccs = [
            CompilableCondition::Ncc(Vec::new()),
            CompilableCondition::Ncc(Vec::new()),
            CompilableCondition::Ncc(Vec::new()),
        ];
        // The terminal, plus an NCC node and its partner per NCC.
        assert_eq!(maximum_new_beta_nodes(&nccs[..n]), 1 + 2 * n);
        std::mem::forget(nccs);
        let patterns = [
            CompilableCondition::Pattern(plain_pattern(kani::any(), kani::any())),
            CompilableCondition::Pattern(plain_pattern(kani::any(), kani::any())),
            CompilableCondition::Pattern(plain_pattern(kani::any(), kani::any())),
        ];
        // The terminal, plus one node per pattern.
        assert_eq!(maximum_new_beta_nodes(&patterns[..n]), 1 + n);
        std::mem::forget(patterns);
    }
}
